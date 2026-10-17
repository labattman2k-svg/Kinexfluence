//! Projects: the unit that imported context is eventually attached to.
//!
//! A project is a name, a slug, and a status. Everything else the schema allows
//! — weights, documents, items — belongs to later PRs and is deliberately not
//! modelled here yet.
//!
//! Every mutation writes an `event_log` row, per the auditability standard:
//! §2 says state changes must be reconstructable, and the project table is one
//! of the five it names.

use chrono::{DateTime, Utc};
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

/// The statuses a project can hold, in lifecycle order.
pub const STATUSES: [&str; 4] = ["recon", "planning", "active", "archived"];

/// The status a project gets when it is created.
pub const DEFAULT_STATUS: &str = "recon";

/// The actor recorded for mutations made through the CLI.
pub const ACTOR_USER: &str = "user";

const ENTITY_TYPE: &str = "project";

/// Shortest slug we accept, in bytes.
const SLUG_MIN: usize = 3;
/// Longest slug we accept, in bytes.
const SLUG_MAX: usize = 64;

/// Column list shared by every read, so the struct and the query cannot drift.
const COLUMNS: &str =
    "id, name, slug, description, brief, status, weights_json, created_at, updated_at";

/// A project row.
///
/// `slug` is `TEXT UNIQUE` and therefore nullable in the schema, but
/// [`create`] always writes one, so a row this module produces always has it.
/// A `NULL` slug from another writer would fail to decode rather than silently
/// becoming an empty string.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub brief: Option<String>,
    pub status: String,
    pub weights_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The input to [`create`].
#[derive(Debug, Clone, Default)]
pub struct CreateProject {
    pub name: String,
    /// Used verbatim when given, and validated. Generated from `name`
    /// otherwise.
    pub slug: Option<String>,
    pub description: Option<String>,
    pub brief: Option<String>,
}

/// Why a project operation failed.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("project not found: {0}")]
    NotFound(String),

    #[error("invalid slug: {0}")]
    InvalidSlug(String),

    #[error("project name is required")]
    EmptyName,

    #[error("duplicate slug: {0}")]
    DuplicateSlug(String),

    /// Not in the brief's enum. `project.status` has no CHECK constraint and
    /// no value list in the schema, so without this a typo would be stored
    /// happily and every `list --status` would then be looking for a value
    /// nothing can produce.
    #[error("invalid status: {0} (expected one of: recon, planning, active, archived)")]
    InvalidStatus(String),
}

/// Create a project and its `created` event.
///
/// Returns the row as stored, so callers get the database-assigned timestamps
/// rather than guessing at them.
pub async fn create(pool: &SqlitePool, input: CreateProject) -> Result<Project, ProjectError> {
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return Err(ProjectError::EmptyName);
    }

    let id = Uuid::now_v7().to_string();
    let mut tx = pool.begin().await?;

    let slug = match input.slug {
        Some(requested) => {
            validate_slug(&requested)?;
            ensure_free(&mut tx, &requested).await?;
            requested
        }
        None => unique_slug(&mut tx, &slugify(&name)).await?,
    };

    sqlx::query(
        "INSERT INTO project (id, name, slug, description, brief, status)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&slug)
    .bind(&input.description)
    .bind(&input.brief)
    .bind(DEFAULT_STATUS)
    .execute(&mut *tx)
    .await?;

    log_event(
        &mut tx,
        &id,
        "created",
        &format!(
            "{{\"name\":{},\"slug\":{}}}",
            json_string(&name),
            json_string(&slug)
        ),
    )
    .await?;

    let project = fetch(&mut *tx, &id)
        .await?
        .ok_or_else(|| ProjectError::NotFound(id.clone()))?;

    tx.commit().await?;
    Ok(project)
}

/// Every project, oldest first.
///
/// The tiebreak on `id` matters: `CURRENT_TIMESTAMP` has one-second resolution,
/// so two projects created in the same second would otherwise come back in
/// whatever order SQLite felt like.
pub async fn list(pool: &SqlitePool) -> Result<Vec<Project>, ProjectError> {
    let projects = sqlx::query_as::<_, Project>(&format!(
        "SELECT {COLUMNS} FROM project ORDER BY created_at, id"
    ))
    .fetch_all(pool)
    .await?;
    Ok(projects)
}

/// The projects in one status, oldest first.
pub async fn list_by_status(pool: &SqlitePool, status: &str) -> Result<Vec<Project>, ProjectError> {
    validate_status(status)?;

    let projects = sqlx::query_as::<_, Project>(&format!(
        "SELECT {COLUMNS} FROM project WHERE status = ? ORDER BY created_at, id"
    ))
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(projects)
}

/// One project by id, or `None`.
pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Project>, ProjectError> {
    let project =
        sqlx::query_as::<_, Project>(&format!("SELECT {COLUMNS} FROM project WHERE id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(project)
}

/// One project by slug, or `None`.
pub async fn find_by_slug(pool: &SqlitePool, slug: &str) -> Result<Option<Project>, ProjectError> {
    let project =
        sqlx::query_as::<_, Project>(&format!("SELECT {COLUMNS} FROM project WHERE slug = ?"))
            .bind(slug)
            .fetch_optional(pool)
            .await?;
    Ok(project)
}

/// Find a project by id or slug.
///
/// A 36-character argument is tried as an id first, then as a slug; anything
/// else is tried as a slug first, then as an id. The two are indistinguishable
/// before the lookup, so trying both costs at most one extra query and never
/// returns the wrong project.
///
/// This lives here rather than in the CLI because two commands need it and the
/// CLI cannot name a `SqlitePool` — `sqlx` is a dev-dependency of that crate.
pub async fn resolve(pool: &SqlitePool, id_or_slug: &str) -> Result<Option<Project>, ProjectError> {
    let by_id_first = id_or_slug.len() == 36;

    let first = if by_id_first {
        get(pool, id_or_slug).await?
    } else {
        find_by_slug(pool, id_or_slug).await?
    };

    if first.is_some() {
        return Ok(first);
    }

    if by_id_first {
        find_by_slug(pool, id_or_slug).await
    } else {
        get(pool, id_or_slug).await
    }
}

/// Change a project's status, and record the transition.
///
/// `updated_at` is set from `CURRENT_TIMESTAMP` in the statement rather than
/// from a Rust-side `Utc::now()`, so the column stays in one textual format.
/// Mixing `2026-10-16 09:41:22` (what the DEFAULT writes) with
/// `2026-10-16T09:41:22Z` (what chrono encodes) would break any `ORDER BY
/// updated_at`, because the two differ at byte 10.
pub async fn update_status(pool: &SqlitePool, id: &str, status: &str) -> Result<(), ProjectError> {
    validate_status(status)?;

    let mut tx = pool.begin().await?;

    let current = fetch(&mut *tx, id)
        .await?
        .ok_or_else(|| ProjectError::NotFound(id.to_owned()))?;

    sqlx::query("UPDATE project SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(status)
        .bind(id)
        .execute(&mut *tx)
        .await?;

    log_event(
        &mut tx,
        id,
        "updated",
        &format!(
            "{{\"from\":{},\"to\":{}}}",
            json_string(&current.status),
            json_string(status)
        ),
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Slug handling
// ---------------------------------------------------------------------------

/// Lowercase a name into slug shape: alphanumerics kept, every run of anything
/// else collapsed to a single hyphen, no leading or trailing hyphen.
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending_dash = false;

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }

    truncate_slug(&out, SLUG_MAX)
}

/// Cut to `max` bytes without leaving a trailing hyphen.
fn truncate_slug(slug: &str, max: usize) -> String {
    let mut out: String = slug.chars().take(max).collect();
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// The slug for a collision attempt: the base on the first try, then `-2`,
/// `-3`, … with the base shortened so the whole thing still fits.
fn with_suffix(base: &str, attempt: u32) -> String {
    if attempt == 1 {
        return truncate_slug(base, SLUG_MAX);
    }
    let suffix = format!("-{attempt}");
    let room = SLUG_MAX.saturating_sub(suffix.len());
    format!("{}{}", truncate_slug(base, room), suffix)
}

fn validate_slug(slug: &str) -> Result<(), ProjectError> {
    if slug.len() < SLUG_MIN || slug.len() > SLUG_MAX {
        return Err(ProjectError::InvalidSlug(format!(
            "{slug:?} must be {SLUG_MIN}-{SLUG_MAX} characters, got {}",
            slug.len()
        )));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(ProjectError::InvalidSlug(format!(
            "{slug:?} must not begin or end with a hyphen"
        )));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ProjectError::InvalidSlug(format!(
            "{slug:?} must be lowercase letters, digits, and hyphens only"
        )));
    }
    Ok(())
}

/// Validate a status against [`STATUSES`].
pub fn validate_status(status: &str) -> Result<(), ProjectError> {
    if STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(ProjectError::InvalidStatus(status.to_owned()))
    }
}

/// Reject an explicit slug that is already taken.
///
/// Deliberately an error rather than silently appending `-2`: the caller asked
/// for *that* slug, and quietly giving them a different one would hide a
/// mistake. Only a generated slug is allowed to shift.
async fn ensure_free(tx: &mut Transaction<'_, Sqlite>, slug: &str) -> Result<(), ProjectError> {
    if slug_taken(tx, slug).await? {
        return Err(ProjectError::DuplicateSlug(slug.to_owned()));
    }
    Ok(())
}

/// Find a free variant of `base`, appending `-2`, `-3`, … as needed.
async fn unique_slug(tx: &mut Transaction<'_, Sqlite>, base: &str) -> Result<String, ProjectError> {
    // A collision is rare and the loop is bounded so a pathological name
    // cannot spin forever.
    const MAX_ATTEMPTS: u32 = 1000;

    for attempt in 1..=MAX_ATTEMPTS {
        let candidate = with_suffix(base, attempt);
        validate_slug(&candidate)?;
        if !slug_taken(tx, &candidate).await? {
            return Ok(candidate);
        }
    }

    Err(ProjectError::DuplicateSlug(base.to_owned()))
}

async fn slug_taken(tx: &mut Transaction<'_, Sqlite>, slug: &str) -> Result<bool, ProjectError> {
    let existing: Option<String> = sqlx::query_scalar("SELECT id FROM project WHERE slug = ?")
        .bind(slug)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(existing.is_some())
}

// ---------------------------------------------------------------------------
// Event log
// ---------------------------------------------------------------------------

async fn log_event(
    tx: &mut Transaction<'_, Sqlite>,
    entity_id: &str,
    action: &str,
    payload_json: &str,
) -> Result<(), ProjectError> {
    sqlx::query(
        "INSERT INTO event_log (id, entity_type, entity_id, action, actor, payload_json)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(ENTITY_TYPE)
    .bind(entity_id)
    .bind(action)
    .bind(ACTOR_USER)
    .bind(payload_json)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn fetch<'e, E>(executor: E, id: &str) -> Result<Option<Project>, ProjectError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let project =
        sqlx::query_as::<_, Project>(&format!("SELECT {COLUMNS} FROM project WHERE id = ?"))
            .bind(id)
            .fetch_optional(executor)
            .await?;
    Ok(project)
}

/// Quote a string as a JSON literal.
///
/// The payloads here are two flat strings, so this avoids pulling `serde_json`
/// into a module that otherwise has no need for it.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
