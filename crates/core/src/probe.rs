//! The probe engine: the user says something matters, the system asks why.
//!
//! A probe is a question asked of the user about one item, together with the
//! answer that came back. It is how a correction enters the system: the
//! scorer ranks a snippet, the user disagrees, and rather than argue with the
//! number the user answers a few questions about what the snippet really was.
//!
//! The answer is what moves the item. A settled probe puts a floor of 0.90
//! under the item's relevance; a provisional one puts 0.70. The floor is the
//! whole point of the mechanism: a heuristic that has never been contradicted
//! will keep making the same mistake, and a user who has said "this matters"
//! should not have to say it twice.
//!
//! The floor is applied when the score is read, not baked into the heuristic.
//! The seven signals still say what they said, and the floor is reported
//! beside them, so the user can see that the system was overruled and by what.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use crate::cart::{self, CartError};
use crate::project::ACTOR_USER;

/// The status of a probe that has been raised and is waiting for an answer.
pub const STATUS_PENDING: &str = "pending";

/// The status of a probe that has been answered but not confirmed.
pub const STATUS_PROVISIONAL: &str = "provisional";

/// The status of a probe that has been answered and confirmed.
pub const STATUS_SETTLED: &str = "settled";

/// The statuses a probe can hold.
///
/// A probe whose status is null has never been answered. The schema allows
/// it, and the module allows it, because a probe is raised before it is
/// answered and the two are separate moments.
pub const STATUSES: [&str; 3] = [STATUS_PENDING, STATUS_PROVISIONAL, STATUS_SETTLED];

/// The relevance floor a settled probe puts under an item's score.
///
/// High enough to clear the auto-select threshold, because a user who has
/// confirmed that a snippet matters has said more than any heuristic could.
pub const FLOOR_SETTLED: f32 = 0.90;

/// The relevance floor a provisional probe puts under an item's score.
///
/// Above the neutral band but below the settled floor: the user has said the
/// snippet matters without saying why they are sure of it.
pub const FLOOR_PROVISIONAL: f32 = 0.70;

/// The entity type recorded in `event_log` for a probe.
const ENTITY_PROBE: &str = "probe";

/// The entity type recorded in `event_log` for a forced item.
const ENTITY_ITEM_PROJECT: &str = "item_project";

/// Column list shared by every read, so the struct and the query cannot drift.
const COLUMNS: &str = "id, item_id, project_id, trigger, questions_json, answers_json, \
                       intent, rationale, impact, confidence, status, created_at";

/// A probe row.
///
/// The struct mirrors the `probe` table, including the two JSON columns, so
/// it can be read straight out of the database. The wire shape the API
/// renders is separate: a client wants the questions and the answers as
/// arrays, not as the text they are stored as.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Probe {
    pub id: String,
    pub item_id: String,
    pub project_id: String,
    pub trigger: String,
    pub questions_json: String,
    pub answers_json: String,
    pub intent: Option<String>,
    pub rationale: Option<String>,
    pub impact: Option<String>,
    pub confidence: Option<String>,
    pub status: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// The input to [`submit_probe`].
///
/// Deserialised straight from the request body, so the wire shape and the
/// domain shape cannot drift apart. A handler that copied these fields into
/// a second struct would be a place for the two to disagree.
///
/// Exactly one of `item_id` and `snippet_id` is given. A snippet is what the
/// user sees, so a client that has one should not have to know that probes
/// are recorded against items. `status` of `None` means the probe has not
/// been answered, and `answers` is empty while that is true.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CreateProbe {
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub snippet_id: Option<String>,
    pub project_id: String,
    /// What raised the probe: a user force-check, a low score, a repeated
    /// import. Free text, because the triggers are the product's business
    /// and not the database's.
    pub trigger: String,
    /// The questions put to the user, and the answers that came back, one per
    /// question.
    pub questions: Vec<String>,
    #[serde(default)]
    pub answers: Vec<String>,
    /// What the user meant the snippet to be for, why they believe that, what
    /// it changes, and how sure they are.
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub impact: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// The input to [`update_probe`].
///
/// Every field is optional, because every field is optional: a probe can be
/// answered without being settled, and settled without changing its answers.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct UpdateProbe {
    #[serde(default)]
    pub answers: Option<Vec<String>>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub impact: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Why a probe operation failed.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("item not found: {0}")]
    ItemNotFound(String),

    #[error("snippet not found: {0}")]
    SnippetNotFound(String),

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("probe not found: {0}")]
    ProbeNotFound(String),

    #[error("a probe needs a trigger: what raised it has to be recorded")]
    EmptyTrigger,

    #[error("a probe needs at least one question: it is the question that is asked")]
    NoQuestions,

    #[error("a probe needs an item or a snippet: it has to be about something")]
    NoTarget,

    #[error("a probe has {questions} question(s) and {answers} answer(s): the answers must line up with the questions")]
    MismatchedAnswers { questions: usize, answers: usize },

    /// Not one of the statuses the module knows.
    ///
    /// The schema has no CHECK constraint on `probe.status`, so without this
    /// a typo would be stored happily and the floor would then silently never
    /// apply -- the worst kind of failure, because the probe looks recorded
    /// and the ranking quietly ignores it.
    #[error("invalid probe status: {0} (expected one of: pending, provisional, settled)")]
    InvalidStatus(String),

    /// A row could not be read back.
    #[error("could not read back the probe: {0}")]
    Corrupt(String),
}

/// The relevance floor a probe's status puts under a score.
///
/// `None` for a probe that has not been answered: a question the user has not
/// yet answered is not evidence, and treating it as evidence would let the
/// system raise a score on the strength of its own question.
pub fn floor_for_status(status: Option<&str>) -> Option<f32> {
    match status {
        Some(STATUS_SETTLED) => Some(FLOOR_SETTLED),
        Some(STATUS_PROVISIONAL) => Some(FLOOR_PROVISIONAL),
        _ => None,
    }
}

/// Reject a status the module does not know.
pub fn validate_status(status: &str) -> Result<(), ProbeError> {
    if STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(ProbeError::InvalidStatus(status.to_owned()))
    }
}

/// Record a probe, and force the item it is about.
///
/// The probe and the forced item are written in one transaction: a probe
/// without its floor would rank the item as though nobody had answered it,
/// and a floor without its probe would have nothing to explain it.
///
/// The floor is applied to the item's stored relevance as well as to the
/// score the API computes, so the two cannot disagree. An item that already
/// scored higher keeps its score: the floor lifts a score, it does not lower
/// one.
pub async fn submit_probe(pool: &SqlitePool, input: CreateProbe) -> Result<Probe, ProbeError> {
    let trigger = input.trigger.trim();
    if trigger.is_empty() {
        return Err(ProbeError::EmptyTrigger);
    }
    if input.questions.is_empty() {
        return Err(ProbeError::NoQuestions);
    }
    if !input.answers.is_empty() && input.answers.len() != input.questions.len() {
        return Err(ProbeError::MismatchedAnswers {
            questions: input.questions.len(),
            answers: input.answers.len(),
        });
    }
    if let Some(status) = input.status.as_deref() {
        validate_status(status)?;
    }

    let mut tx = pool.begin().await?;
    let item_id = resolve_target(&mut tx, &input).await?;

    if !exists(&mut tx, "project", &input.project_id).await? {
        return Err(ProbeError::ProjectNotFound(input.project_id));
    }

    let id = Uuid::now_v7().to_string();
    let questions_json = serde_json::to_string(&input.questions)
        .map_err(|cause| sqlx::Error::Protocol(cause.to_string()))?;
    let answers_json = serde_json::to_string(&input.answers)
        .map_err(|cause| sqlx::Error::Protocol(cause.to_string()))?;

    sqlx::query(
        "INSERT INTO probe (id, item_id, project_id, trigger, questions_json, answers_json,
                            intent, rationale, impact, confidence, status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&item_id)
    .bind(&input.project_id)
    .bind(trigger)
    .bind(&questions_json)
    .bind(&answers_json)
    .bind(&input.intent)
    .bind(&input.rationale)
    .bind(&input.impact)
    .bind(&input.confidence)
    .bind(&input.status)
    .execute(&mut *tx)
    .await?;

    let floor = floor_for_status(input.status.as_deref());
    force_item(&mut tx, &item_id, &input.project_id, floor).await?;

    log_event(
        &mut tx,
        ENTITY_PROBE,
        &id,
        "created",
        &serde_json::json!({
            "item_id": item_id,
            "project_id": input.project_id,
            "trigger": trigger,
            "status": input.status,
            "floor": floor,
        })
        .to_string(),
    )
    .await?;

    log_event(
        &mut tx,
        ENTITY_ITEM_PROJECT,
        &item_id,
        "probe_floor_applied",
        &serde_json::json!({
            "project_id": input.project_id,
            "probe_id": id,
            "floor": floor,
        })
        .to_string(),
    )
    .await?;

    let probe = fetch(&mut tx, &id)
        .await?
        .ok_or_else(|| ProbeError::Corrupt(id.clone()))?;

    tx.commit().await?;
    Ok(probe)
}

/// Answer a probe, and re-apply its floor.
///
/// This is the second half of the mechanism: a probe is raised with
/// questions, and the answers arrive later. The floor follows the status,
/// so answering a probe is what actually moves the item.
///
/// The answers have to line up with the questions that were asked. A probe
/// with fewer answers than questions is not partially answered -- it is
/// mismatched, and accepting it would store an answer against the wrong
/// question.
pub async fn update_probe(
    pool: &SqlitePool,
    id: &str,
    input: UpdateProbe,
) -> Result<Probe, ProbeError> {
    if let Some(status) = input.status.as_deref() {
        validate_status(status)?;
    }

    let mut tx = pool.begin().await?;
    let current = fetch(&mut tx, id)
        .await?
        .ok_or_else(|| ProbeError::ProbeNotFound(id.to_owned()))?;

    if let Some(answers) = input.answers.as_ref() {
        let questions: Vec<String> =
            serde_json::from_str(&current.questions_json).unwrap_or_default();
        if !answers.is_empty() && answers.len() != questions.len() {
            return Err(ProbeError::MismatchedAnswers {
                questions: questions.len(),
                answers: answers.len(),
            });
        }
    }

    let answers_json = match input.answers.as_ref() {
        Some(answers) => serde_json::to_string(answers)
            .map_err(|cause| sqlx::Error::Protocol(cause.to_string()))?,
        None => current.answers_json.clone(),
    };

    // A status that is not given stays as it was. Changing the answers of a
    // settled probe does not unsettle it: the user has already said what
    // they meant, and the floor under the item should not wobble because
    // they edited a word.
    let status = input.status.clone().or(current.status.clone());

    sqlx::query(
        "UPDATE probe SET answers_json = ?, intent = ?, rationale = ?, impact = ?,
                          confidence = ?, status = ?
         WHERE id = ?",
    )
    .bind(&answers_json)
    .bind(input.intent.or(current.intent.clone()))
    .bind(input.rationale.or(current.rationale.clone()))
    .bind(input.impact.or(current.impact.clone()))
    .bind(input.confidence.or(current.confidence.clone()))
    .bind(&status)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let floor = floor_for_status(status.as_deref());
    force_item(&mut tx, &current.item_id, &current.project_id, floor).await?;

    log_event(
        &mut tx,
        ENTITY_PROBE,
        id,
        "answered",
        &serde_json::json!({
            "status": status,
            "floor": floor,
        })
        .to_string(),
    )
    .await?;

    let probe = fetch(&mut tx, id)
        .await?
        .ok_or_else(|| ProbeError::Corrupt(id.to_owned()))?;

    tx.commit().await?;
    Ok(probe)
}

/// Work out which item a probe is about.
///
/// A probe is recorded against an item, and an item is a snippet's content
/// copied into the cart. A client that has a snippet does not have an item
/// until the snippet has been added to a cart, so a probe named by its
/// snippet gets the item created for it -- the same way `cart::add_to_cart`
/// does. A probe about something that has never been in a cart is a probe
/// about a snippet the user wants, which is exactly the case the mechanism
/// exists for.
async fn resolve_target(
    tx: &mut Transaction<'_, Sqlite>,
    input: &CreateProbe,
) -> Result<String, ProbeError> {
    if let Some(item_id) = input.item_id.as_ref().filter(|id| !id.is_empty()) {
        if !exists(tx, "item", item_id).await? {
            return Err(ProbeError::ItemNotFound(item_id.clone()));
        }
        return Ok(item_id.clone());
    }

    let snippet_id = input
        .snippet_id
        .as_ref()
        .filter(|id| !id.is_empty())
        .ok_or(ProbeError::NoTarget)?;

    // The item is created if it is not there yet, and it is the cart's own
    // rule that decides: there must be exactly one item per snippet, or a
    // snippet could be in two places at once.
    cart::item_for_snippet(tx, snippet_id)
        .await
        .map_err(|cause| match cause {
            CartError::SnippetNotFound(id) => ProbeError::SnippetNotFound(id),
            other => ProbeError::Db(sqlx::Error::Protocol(other.to_string())),
        })
}

/// The probes that have not been settled, oldest first.
///
/// "Not settled" rather than "not answered": a probe the user has answered
/// but not confirmed is still in flight, and it is still in front of them
/// until they confirm it. Anything else would leave a question the user
/// already answered sitting in their queue forever.
///
/// The tiebreak on `id` matters for the same reason it does in `project.rs`:
/// `CURRENT_TIMESTAMP` has one-second resolution, so two probes raised in
/// the same second would otherwise come back in whatever order SQLite felt
/// like.
///
/// `project_id` narrows the list to one project, so a client showing one
/// project's queue does not fetch every project's. The filter lives here
/// rather than in the client, so the rule is in one place.
pub async fn list_pending(
    pool: &SqlitePool,
    project_id: Option<&str>,
) -> Result<Vec<Probe>, ProbeError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM probe
         WHERE (status IS NULL OR status != ?) AND (? IS NULL OR project_id = ?)
         ORDER BY created_at, id"
    );
    let probes = sqlx::query_as::<_, Probe>(&sql)
        .bind(STATUS_SETTLED)
        .bind(project_id)
        .bind(project_id)
        .fetch_all(pool)
        .await?;
    Ok(probes)
}

/// The highest floor any probe puts under each snippet in a project.
///
/// A probe is recorded against an item, and an item points at a snippet, so
/// this is the join from the probe floor to the snippets the scorer ranks.
/// A snippet with several probes gets the highest of their floors: the user
/// was most sure about the one with the strongest answer.
///
/// Snippets with no probe are absent from the map rather than mapped to
/// zero, so a caller cannot mistake "no floor" for "a floor of zero".
pub async fn floors_by_snippet(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<HashMap<String, f32>, ProbeError> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT i.snippet_id, p.status
         FROM probe p
         JOIN item i ON i.id = p.item_id
         WHERE p.project_id = ? AND i.snippet_id IS NOT NULL",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    let mut floors: HashMap<String, f32> = HashMap::new();
    for (snippet_id, status) in rows {
        if let Some(floor) = floor_for_status(status.as_deref()) {
            let highest = floors.entry(snippet_id).or_insert(floor);
            *highest = highest.max(floor);
        }
    }
    Ok(floors)
}

/// Whether a row exists in a table with an `id` column.
async fn exists(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    id: &str,
) -> Result<bool, ProbeError> {
    let sql = format!("SELECT 1 FROM {table} WHERE id = ?");
    let found: Option<(i32,)> = sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(found.is_some())
}

/// Put the probe floor under the item's stored relevance, and mark it forced.
///
/// The row is created if it is not there yet: a probe is the user saying
/// this item matters to this project, and `item_project` is where that
/// relationship is recorded. Without the row there would be nowhere to keep
/// the `user_forced` flag or the relevance the floor produced.
///
/// An item that is already in the project keeps the status it had, so
/// probing a committed item does not drag it back into the cart.
async fn force_item(
    tx: &mut Transaction<'_, Sqlite>,
    item_id: &str,
    project_id: &str,
    floor: Option<f32>,
) -> Result<(), ProbeError> {
    sqlx::query(
        "INSERT INTO item_project (item_id, project_id, status, relevance_score, user_forced, added_at)
         VALUES (?, ?, 'cart', ?, 1, ?)
         ON CONFLICT(item_id, project_id) DO UPDATE SET
             user_forced = 1,
             relevance_score = CASE
                 WHEN excluded.relevance_score IS NULL THEN item_project.relevance_score
                 WHEN item_project.relevance_score IS NULL THEN excluded.relevance_score
                 ELSE MAX(item_project.relevance_score, excluded.relevance_score)
             END",
    )
    .bind(item_id)
    .bind(project_id)
    .bind(floor)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Write one `event_log` row.
///
/// The standard names `probe` and `item_project` among the entities whose
/// mutations must be reconstructable, and a probe submission mutates both,
/// so it writes both.
async fn log_event(
    tx: &mut Transaction<'_, Sqlite>,
    entity_type: &str,
    entity_id: &str,
    action: &str,
    payload_json: &str,
) -> Result<(), ProbeError> {
    sqlx::query(
        "INSERT INTO event_log (id, entity_type, entity_id, action, actor, payload_json)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(entity_type)
    .bind(entity_id)
    .bind(action)
    .bind(ACTOR_USER)
    .bind(payload_json)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// One probe by id.
async fn fetch(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<Option<Probe>, ProbeError> {
    let sql = format!("SELECT {COLUMNS} FROM probe WHERE id = ?");
    let probe = sqlx::query_as::<_, Probe>(&sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(probe)
}
