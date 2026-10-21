//! Tests for the probe engine.
//!
//! A probe is the mechanism by which a user tells the system that a snippet
//! matters and the system was wrong about it. What matters is therefore not
//! only that the probe is recorded, but that the recording *does something*:
//! the floor has to reach the score the user sees.
//!
//! The tests are driven against a real migrated database, because what is
//! being tested is how `probe.rs` and SQLite behave together -- the floor
//! lands in `item_project`, and a test against a mock would not notice if it
//! landed in the wrong place.

use kfx_core::probe::{
    floor_for_status, list_pending, submit_probe, update_probe, CreateProbe, ProbeError,
    UpdateProbe, FLOOR_PROVISIONAL, FLOOR_SETTLED, STATUS_PENDING, STATUS_PROVISIONAL,
    STATUS_SETTLED,
};
use sqlx::SqlitePool;

/// Create a project and return its id.
async fn make_project(pool: &SqlitePool, name: &str) -> String {
    let id = uuid::Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO project (id, name, slug) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(name.to_lowercase().replace(' ', "-"))
        .execute(pool)
        .await
        .expect("the project should be inserted");
    id
}

/// Create a snippet and return its id.
///
/// The chain is source -> conversation -> message -> snippet, because that is
/// what the foreign keys demand.
async fn make_snippet(pool: &SqlitePool, text: &str) -> String {
    sqlx::query("INSERT OR IGNORE INTO source (id, name) VALUES ('source-1', 'source-1')")
        .execute(pool)
        .await
        .expect("the source should be inserted");

    let conversation_id = uuid::Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO conversation (id, source_id, title) VALUES (?, 'source-1', 'Test')")
        .bind(&conversation_id)
        .execute(pool)
        .await
        .expect("the conversation should be inserted");

    let message_id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO message (id, conversation_id, role, content_json) VALUES (?, ?, 'user', '{}')",
    )
    .bind(&message_id)
    .bind(&conversation_id)
    .execute(pool)
    .await
    .expect("the message should be inserted");

    let snippet_id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO snippet (id, message_id, block_index, kind, text, hash) VALUES (?, ?, 0, 'text', ?, ?)",
    )
    .bind(&snippet_id)
    .bind(&message_id)
    .bind(text)
    .bind(format!("hash-{snippet_id}"))
    .execute(pool)
    .await
    .expect("the snippet should be inserted");

    snippet_id
}

/// A project and a snippet to probe, made in one call.
///
/// Most tests need both and neither cares about the ids, so making them
/// together keeps the tests to one line of setup each.
async fn make_target(pool: &SqlitePool) -> (String, String) {
    let project_id = make_project(pool, "Probe Project").await;
    let snippet_id = make_snippet(pool, "a snippet worth keeping").await;
    (project_id, snippet_id)
}

/// A probe about a snippet, with a settled status.
fn settled_probe(snippet_id: &str, project_id: &str) -> CreateProbe {
    CreateProbe {
        item_id: None,
        snippet_id: Some(snippet_id.to_string()),
        project_id: project_id.to_string(),
        trigger: "user_force_check".to_string(),
        questions: vec!["What was this for?".to_string()],
        answers: vec!["The storage decision".to_string()],
        intent: Some("records a decision".to_string()),
        rationale: None,
        impact: None,
        confidence: Some("high".to_string()),
        status: Some(STATUS_SETTLED.to_string()),
    }
}

/// The stored relevance and the forced flag of an item in a project.
async fn stored_relevance(
    pool: &SqlitePool,
    item_id: &str,
    project_id: &str,
) -> (i64, Option<f64>) {
    let row: Option<(i64, Option<f64>)> = sqlx::query_as(
        "SELECT user_forced, relevance_score FROM item_project WHERE item_id = ? AND project_id = ?",
    )
    .bind(item_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .expect("the item_project row should be readable");
    row.expect("the item should be in the project")
}

// ---------------------------------------------------------------------------
// The floors
//
// The floor is the whole point. A probe that is recorded but changes nothing
// is a probe the system has learned to ignore, so the numbers themselves
// get their own tests.
// ---------------------------------------------------------------------------

#[test]
fn a_settled_probe_floors_at_ninety() {
    assert_eq!(
        floor_for_status(Some(STATUS_SETTLED)),
        Some(FLOOR_SETTLED),
        "a confirmed answer is worth the most the floor can give"
    );
    assert_eq!(FLOOR_SETTLED, 0.90);
}

#[test]
fn a_provisional_probe_floors_at_seventy() {
    assert_eq!(
        floor_for_status(Some(STATUS_PROVISIONAL)),
        Some(FLOOR_PROVISIONAL)
    );
    assert_eq!(FLOOR_PROVISIONAL, 0.70);
}

#[test]
fn an_unanswered_probe_floors_nowhere() {
    // A question the user has not answered is not evidence. Treating it as
    // evidence would let the system raise a score on the strength of its own
    // question, which is the opposite of what a probe is for.
    assert_eq!(floor_for_status(None), None);
    assert_eq!(floor_for_status(Some(STATUS_PENDING)), None);
}

// ---------------------------------------------------------------------------
// Submitting
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn submitting_a_probe_writes_the_row(pool: SqlitePool) {
    let (project_id, snippet_id) = make_target(&pool).await;

    let probe = submit_probe(&pool, settled_probe(&snippet_id, &project_id))
        .await
        .expect("the probe should be accepted");

    assert!(!probe.id.is_empty(), "the probe should have an id");
    assert_eq!(probe.project_id, project_id);
    assert_eq!(probe.trigger, "user_force_check");
    assert_eq!(probe.status.as_deref(), Some(STATUS_SETTLED));
    assert_eq!(probe.intent.as_deref(), Some("records a decision"));

    // The row is really in the database, not just in the returned struct.
    let stored: Option<(String, String)> =
        sqlx::query_as("SELECT trigger, status FROM probe WHERE id = ?")
            .bind(&probe.id)
            .fetch_optional(&pool)
            .await
            .expect("the probe should be readable");
    assert_eq!(
        stored,
        Some(("user_force_check".to_string(), STATUS_SETTLED.to_string()))
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn submitting_a_probe_forces_the_item(pool: SqlitePool) {
    let (project_id, snippet_id) = make_target(&pool).await;

    let probe = submit_probe(&pool, settled_probe(&snippet_id, &project_id))
        .await
        .expect("the probe should be accepted");

    // The item is created for the snippet, and the relationship to the
    // project records both that the user forced it and what it is worth.
    let (user_forced, relevance) = stored_relevance(&pool, &probe.item_id, &project_id).await;

    assert_eq!(user_forced, 1, "the item should be marked as forced");
    assert_eq!(
        relevance,
        Some(FLOOR_SETTLED as f64),
        "the stored relevance should be the floor"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn a_probe_writes_the_event_log(pool: SqlitePool) {
    let (project_id, snippet_id) = make_target(&pool).await;

    let probe = submit_probe(&pool, settled_probe(&snippet_id, &project_id))
        .await
        .expect("the probe should be accepted");

    // The auditability standard names `probe` and `item_project` among the
    // entities whose mutations must be reconstructable, and a submission
    // mutates both.
    for (entity_type, entity_id) in [("probe", &probe.id), ("item_project", &probe.item_id)] {
        let events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM event_log WHERE entity_type = ? AND entity_id = ?",
        )
        .bind(entity_type)
        .bind(entity_id)
        .fetch_one(&pool)
        .await
        .expect("the event log should be readable");
        assert_eq!(events, 1, "the {entity_type} should leave one event");
    }
}

/// Every way a submission can be refused.
///
/// The rejections are listed together because they are one rule each: the
/// probe has to name something, in a project that exists, with a trigger,
/// with at least one question, with answers that line up, and with a status
/// the module knows.
#[sqlx::test(migrations = "../../schema/migrations")]
async fn a_malformed_probe_is_rejected(pool: SqlitePool) {
    let (project_id, snippet_id) = make_target(&pool).await;

    // A missing item.
    let mut input = settled_probe(&snippet_id, &project_id);
    input.snippet_id = None;
    input.item_id = Some("item-that-does-not-exist".to_string());
    assert!(matches!(
        submit_probe(&pool, input).await,
        Err(ProbeError::ItemNotFound(_))
    ));

    // A missing project.
    let input = settled_probe(&snippet_id, "project-that-does-not-exist");
    assert!(matches!(
        submit_probe(&pool, input).await,
        Err(ProbeError::ProjectNotFound(_))
    ));

    // No questions.
    let mut input = settled_probe(&snippet_id, &project_id);
    input.questions = Vec::new();
    assert!(matches!(
        submit_probe(&pool, input).await,
        Err(ProbeError::NoQuestions)
    ));

    // No trigger.
    let mut input = settled_probe(&snippet_id, &project_id);
    input.trigger = "   ".to_string();
    assert!(matches!(
        submit_probe(&pool, input).await,
        Err(ProbeError::EmptyTrigger)
    ));

    // Answers that do not line up with the questions.
    let mut input = settled_probe(&snippet_id, &project_id);
    input.questions = vec!["One?".to_string(), "Two?".to_string()];
    input.answers = vec!["Only one answer".to_string()];
    match submit_probe(&pool, input).await {
        Err(ProbeError::MismatchedAnswers { questions, answers }) => {
            assert_eq!(questions, 2);
            assert_eq!(answers, 1);
        }
        other => panic!("mismatched answers should be rejected, got {other:?}"),
    }

    // A status the module does not know.
    let mut input = settled_probe(&snippet_id, &project_id);
    input.status = Some("probably fine".to_string());
    assert!(matches!(
        submit_probe(&pool, input).await,
        Err(ProbeError::InvalidStatus(_))
    ));

    // And nothing was written.
    let probes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM probe")
        .fetch_one(&pool)
        .await
        .expect("the probe table should be readable");
    assert_eq!(probes, 0, "a refused probe must not leave a row");
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_pending_returns_the_probes_that_are_not_settled(pool: SqlitePool) {
    let (project_id, snippet_id) = make_target(&pool).await;

    // An unanswered probe, and one that has been answered but not confirmed.
    let mut pending = settled_probe(&snippet_id, &project_id);
    pending.status = None;
    pending.answers = Vec::new();
    submit_probe(&pool, pending)
        .await
        .expect("the probe should be accepted");

    let mut provisional = settled_probe(&snippet_id, &project_id);
    provisional.status = Some(STATUS_PROVISIONAL.to_string());
    submit_probe(&pool, provisional)
        .await
        .expect("the probe should be accepted");

    // And one that has been settled, which is no longer in front of the user.
    submit_probe(&pool, settled_probe(&snippet_id, &project_id))
        .await
        .expect("the probe should be accepted");

    let pending_probes = list_pending(&pool)
        .await
        .expect("the probes should be listable");

    assert_eq!(
        pending_probes.len(),
        2,
        "a settled probe is done and should not be in the queue"
    );
    assert!(
        pending_probes
            .iter()
            .all(|probe| probe.status != Some(STATUS_SETTLED.to_string())),
        "a settled probe should never be pending: {pending_probes:?}"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_pending_is_empty_when_there_is_nothing_to_answer(pool: SqlitePool) {
    let probes = list_pending(&pool)
        .await
        .expect("the probes should be listable");
    assert!(
        probes.is_empty(),
        "an empty probe table is an empty list, not an error: {probes:?}"
    );
}

// ---------------------------------------------------------------------------
// Answering
//
// The probe is raised with questions, and the answers arrive later. The
// answers are what actually move the item, so this is where the floor is
// really applied.
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn answering_a_probe_applies_the_floor(pool: SqlitePool) {
    let (project_id, snippet_id) = make_target(&pool).await;

    // Raised, with nothing but questions.
    let mut input = settled_probe(&snippet_id, &project_id);
    input.status = None;
    input.answers = Vec::new();
    let raised = submit_probe(&pool, input)
        .await
        .expect("the probe should be accepted");

    // Nothing is forced yet: an unanswered probe floors nowhere.
    let (user_forced, relevance) = stored_relevance(&pool, &raised.item_id, &project_id).await;
    assert_eq!(user_forced, 1, "the item is still marked as forced");
    assert_eq!(
        relevance, None,
        "an unanswered probe floors nowhere, so the score is left alone"
    );

    // Answered and settled.
    let answered = update_probe(
        &pool,
        &raised.id,
        UpdateProbe {
            answers: Some(vec!["The storage decision".to_string()]),
            status: Some(STATUS_SETTLED.to_string()),
            ..UpdateProbe::default()
        },
    )
    .await
    .expect("the probe should be answerable");

    assert_eq!(answered.status.as_deref(), Some(STATUS_SETTLED));

    let (user_forced, relevance) = stored_relevance(&pool, &raised.item_id, &project_id).await;
    assert_eq!(user_forced, 1);
    assert_eq!(
        relevance,
        Some(FLOOR_SETTLED as f64),
        "answering the probe should have put the floor under the item"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn answers_must_line_up_with_the_questions(pool: SqlitePool) {
    let (project_id, snippet_id) = make_target(&pool).await;

    let mut input = settled_probe(&snippet_id, &project_id);
    input.questions = vec!["One?".to_string(), "Two?".to_string()];
    input.answers = Vec::new();
    input.status = None;
    let raised = submit_probe(&pool, input)
        .await
        .expect("the probe should be accepted");

    match update_probe(
        &pool,
        &raised.id,
        UpdateProbe {
            answers: Some(vec!["Only one".to_string()]),
            status: Some(STATUS_SETTLED.to_string()),
            ..UpdateProbe::default()
        },
    )
    .await
    {
        Err(ProbeError::MismatchedAnswers { questions, answers }) => {
            assert_eq!(questions, 2);
            assert_eq!(answers, 1);
        }
        Err(other) => panic!("mismatched answers should be rejected, got {other:?}"),
        Ok(_) => panic!("a probe with mismatched answers should not be accepted"),
    }
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn answering_a_missing_probe_is_rejected(pool: SqlitePool) {
    match update_probe(
        &pool,
        "probe-that-does-not-exist",
        UpdateProbe {
            status: Some(STATUS_SETTLED.to_string()),
            ..UpdateProbe::default()
        },
    )
    .await
    {
        Err(ProbeError::ProbeNotFound(_)) => {}
        Err(other) => panic!("a missing probe should report as not found, got {other:?}"),
        Ok(_) => panic!("answering a missing probe should not succeed"),
    }
}
