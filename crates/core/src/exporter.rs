//! The context pack: a project's committed context, rendered as Markdown.
//!
//! A context pack is what the user takes away. The rest of the system gets
//! the right fragments in front of the user; this module hands them to
//! something else — another model, a colleague, a document — in a form that
//! reads without the application. Markdown, because it is both plain text and
//! structured: it renders in a terminal, in a browser, in a review comment,
//! and in a prompt.
//!
//! A pack holds the committed items of a project and nothing else. An item in
//! the cart is a candidate; only an item the user has committed is part of
//! the project's context. Exporting the cart instead would export the user's
//! uncertainty.
//!
//! `item.kind` says whether a fragment is prose or code — it is the snippet's
//! kind, not a statement about what the fragment is about — so the section a
//! fragment belongs under is decided here. The rules are deliberately crude:
//! a pack that says "this looks like a decision" is a prompt for the reader
//! to check, not a claim that can be wrong.

use sqlx::SqlitePool;

use crate::project::{self, ProjectError};

/// The status an item has to hold to be part of the pack.
pub const STATUS_COMMITTED: &str = "committed";

/// The heading the code section carries.
const CODE_SECTION: &str = "Code";

/// The prose sections, in the order a reader wants them: what was decided,
/// what cannot change, what is still open, and everything else.
const PROSE_SECTIONS: [&str; 4] = ["Decisions", "Constraints", "Open Questions", "Notes"];

/// Words that mark a fragment as a decision.
///
/// Matched against the lowercased text, as substrings, so "we decided" and
/// "Decisions:" both land.
const DECISION_WORDS: [&str; 10] = [
    "decided",
    "decision",
    "we will",
    "we'll",
    "going with",
    "chose",
    "chosen",
    "agreed",
    "adopted",
    "settled on",
];

/// Words that mark a fragment as a constraint.
const CONSTRAINT_WORDS: [&str; 11] = [
    "must",
    "cannot",
    "can't",
    "required",
    "forbidden",
    "not allowed",
    "only if",
    "never",
    "always",
    "at least",
    "at most",
];

/// Why an export failed.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("project error: {0}")]
    Project(#[from] ProjectError),

    #[error("project not found: {0}")]
    ProjectNotFound(String),
}

/// A committed item, as the pack needs it.
#[derive(Debug, sqlx::FromRow)]
struct CommittedItem {
    kind: String,
    text: String,
    metadata_json: Option<String>,
}

/// One section of the pack: a heading, and the items under it.
struct Section {
    title: String,
    items: Vec<PackItem>,
}

/// One item in a section.
struct PackItem {
    text: String,
    /// Whether the item is code, and so is fenced rather than bulleted.
    code: bool,
    /// The language of a code item, for the fence's info string.
    language: Option<String>,
}

/// The project's committed context, as a Markdown document.
///
/// The project is named by id or slug, and the lookup is the same one the rest
/// of the product uses: a 36-character argument is tried as an id first,
/// anything else as a slug first. A project that does not exist is an error
/// rather than an empty pack — an empty pack is a valid answer for a project
/// with nothing committed, and the two must not look alike.
pub async fn export_project(pool: &SqlitePool, id_or_slug: &str) -> Result<String, ExportError> {
    let project = project::resolve(pool, id_or_slug)
        .await?
        .ok_or_else(|| ExportError::ProjectNotFound(id_or_slug.to_owned()))?;

    let items = committed_items(pool, &project.id).await?;

    Ok(render(&project.name, project.brief.as_deref(), items))
}

/// The committed items of a project, oldest commit first.
///
/// The tiebreak on `item_id` matters: `CURRENT_TIMESTAMP` has one-second
/// resolution, so two items committed in the same second would otherwise come
/// back in whatever order SQLite felt like.
async fn committed_items(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<Vec<CommittedItem>, ExportError> {
    let items = sqlx::query_as::<_, CommittedItem>(
        "SELECT i.kind, i.text, i.metadata_json
         FROM item_project ip
         JOIN item i ON i.id = ip.item_id
         WHERE ip.project_id = ? AND ip.status = ?
         ORDER BY ip.committed_at, ip.item_id",
    )
    .bind(project_id)
    .bind(STATUS_COMMITTED)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

/// The language an item carries, if any.
///
/// The cart copies the snippet's language onto the item when it materialises
/// it, so the item is where the language lives. Reading it from there rather
/// than from the snippet keeps the pack a question about the project's
/// context and not about the fragments it was built from.
fn language_of(metadata_json: Option<&str>) -> Option<String> {
    let metadata = metadata_json?;
    let value: serde_json::Value = serde_json::from_str(metadata).ok()?;
    value.get("language")?.as_str().map(str::to_owned)
}

/// Render the pack.
///
/// Pure, so the shape of the document can be tested without a database.
fn render(name: &str, brief: Option<&str>, items: Vec<CommittedItem>) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {name}\n\n"));

    // The brief is what the project is for. Without it the pack starts with a
    // heading and nothing else, which reads like a broken file.
    if let Some(brief) = brief.map(str::trim).filter(|brief| !brief.is_empty()) {
        out.push_str(brief);
        out.push_str("\n\n");
    }

    if items.is_empty() {
        out.push_str("_No items have been committed to this project yet._\n");
        return out;
    }

    for section in sections(items) {
        if section.items.is_empty() {
            continue;
        }
        out.push_str(&format!("## {}\n\n", section.title));
        for item in section.items {
            if item.code {
                out.push_str(&fenced(&item.text, item.language.as_deref().unwrap_or("")));
            } else {
                out.push_str(&bullet(&item.text));
            }
        }
        out.push('\n');
    }

    out
}

/// The pack's sections, in order, with the items under each.
fn sections(items: Vec<CommittedItem>) -> Vec<Section> {
    let mut out: Vec<Section> = PROSE_SECTIONS
        .iter()
        .map(|title| Section {
            title: (*title).to_owned(),
            items: Vec::new(),
        })
        .collect();

    // The code section is last: it is the bulkiest and the least often the
    // first thing a reader wants.
    let mut code = Section {
        title: CODE_SECTION.to_owned(),
        items: Vec::new(),
    };

    for item in items {
        if item.kind == "code" {
            code.items.push(PackItem {
                text: item.text,
                code: true,
                language: language_of(item.metadata_json.as_deref()),
            });
            continue;
        }

        let title = section_for(&item.text);
        match out.iter_mut().find(|section| section.title == title) {
            Some(section) => section.items.push(PackItem {
                text: item.text,
                code: false,
                language: None,
            }),
            // `section_for` only ever names a section that is in `out`, but a
            // future rule that names a new one must not silently drop items.
            None => out.push(Section {
                title: title.to_owned(),
                items: vec![PackItem {
                    text: item.text,
                    code: false,
                    language: None,
                }],
            }),
        }
    }

    out.push(code);
    out
}

/// The section a piece of prose belongs under.
///
/// A question is a question, whatever else it says. After that, a fragment
/// that says a choice was made is a decision, and one that says something
/// cannot vary is a constraint. Everything else is a note.
fn section_for(text: &str) -> &'static str {
    if text.contains('?') {
        return "Open Questions";
    }

    let lower = text.to_lowercase();
    if DECISION_WORDS.iter().any(|word| lower.contains(word)) {
        return "Decisions";
    }
    if CONSTRAINT_WORDS.iter().any(|word| lower.contains(word)) {
        return "Constraints";
    }
    "Notes"
}

/// One prose item, as a Markdown list item.
///
/// A fragment that runs to several lines stays one list item: the
/// continuation lines are indented, so the reader sees one bullet rather than
/// a paragraph that happens to follow a bullet.
fn bullet(text: &str) -> String {
    let mut out = String::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            out.push_str("- ");
        } else {
            out.push_str("  ");
        }
        out.push_str(line);
        out.push('\n');
    }
    // An empty fragment has no lines, and would leave no bullet at all — the
    // item would vanish from the pack without anyone being told.
    if out.is_empty() {
        out.push_str("- \n");
    }
    out
}

/// One code item, as a fenced block.
///
/// The fence is longer than any run of backticks in the text, because a
/// shorter one would end the block early and the rest of the code would be
/// read as prose.
fn fenced(text: &str, language: &str) -> String {
    let fence = "`".repeat(fence_length(text));
    let mut out = String::new();
    out.push_str(&fence);
    out.push_str(language);
    out.push('\n');
    out.push_str(text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&fence);
    out.push('\n');
    out
}

/// The length of the fence that will hold a piece of code.
///
/// CommonMark's rule: the fence has to be at least three characters, and
/// longer than the longest run of backticks inside the block.
fn fence_length(text: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for ch in text.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest.max(2) + 1
}
