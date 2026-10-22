//! Project entity tests.
//!
//! Each test runs against a real migrated database via `#[sqlx::test]`. The
//! behaviour under test is mostly what the schema and the SQL do — the UNIQUE
//! slug constraint, the DEFAULT timestamps, the event log — so a mock would
//! only confirm the mock.

use kfx_core::project::{
    create, find_by_slug, get, list, list_by_status, resolve, resolve_id, update_status,
    CreateProject, ProjectError,
};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

fn input(name: &str) -> CreateProject {
    CreateProject {
        name: name.to_owned(),
        ..CreateProject::default()
    }
}

async fn event_count(pool: &SqlitePool, entity_id: &str, action: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM event_log WHERE entity_id = ? AND action = ?")
        .bind(entity_id)
        .bind(action)
        .fetch_one(pool)
        .await
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_inserts_row(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let project = create(
        &pool,
        CreateProject {
            name: "Kilimo Data Residency".to_owned(),
            slug: None,
            description: Some("Data residency strategy.".to_owned()),
            brief: Some("Regulatory requirement.".to_owned()),
        },
    )
    .await?;

    assert_eq!(project.name, "Kilimo Data Residency");
    assert_eq!(project.slug, "kilimo-data-residency");
    assert_eq!(
        project.description.as_deref(),
        Some("Data residency strategy.")
    );
    assert_eq!(project.brief.as_deref(), Some("Regulatory requirement."));
    assert_eq!(
        project.status, "recon",
        "a new project starts in recon, per the schema default"
    );
    assert_eq!(project.weights_json, None);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 1);

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_generates_uuid_v7(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let project = create(&pool, input("One")).await?;

    let parsed = Uuid::parse_str(&project.id).expect("the id should be a uuid");
    assert_eq!(
        parsed.get_version_num(),
        7,
        "ids are uuid v7 so they sort by creation time, matching the normalizer"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_slug_from_name(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let project = create(&pool, input("  Kilimo   Data Residency!  ")).await?;

    assert_eq!(
        project.slug, "kilimo-data-residency",
        "runs of non-alphanumerics collapse to one hyphen and the ends are trimmed"
    );
    assert_eq!(
        project.name, "Kilimo   Data Residency!",
        "the name keeps its interior whitespace; only the ends are trimmed"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_explicit_slug_used(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let project = create(
        &pool,
        CreateProject {
            name: "Anything At All".to_owned(),
            slug: Some("chosen-slug".to_owned()),
            ..CreateProject::default()
        },
    )
    .await?;

    assert_eq!(project.slug, "chosen-slug");

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_rejects_empty_name(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    for name in ["", "   ", "\t\n"] {
        let error = create(&pool, input(name))
            .await
            .expect_err("a project needs a name");
        assert!(
            matches!(error, ProjectError::EmptyName),
            "expected EmptyName for {name:?}, got {error:?}"
        );
    }

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0, "a rejected create must not leave a row");

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_rejects_invalid_slug(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    // Each of these breaks exactly one rule: too short, uppercase, a space, and
    // a leading hyphen.
    for slug in ["ab", "UpperCase", "has space", "-leading"] {
        let error = create(
            &pool,
            CreateProject {
                name: "Fine Name".to_owned(),
                slug: Some(slug.to_owned()),
                ..CreateProject::default()
            },
        )
        .await
        .expect_err("invalid slugs must be refused");

        assert!(
            matches!(error, ProjectError::InvalidSlug(_)),
            "expected InvalidSlug for {slug:?}, got {error:?}"
        );
    }

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_handles_slug_collision(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let first = create(&pool, input("Same Name")).await?;
    let second = create(&pool, input("Same Name")).await?;
    let third = create(&pool, input("Same Name")).await?;

    assert_eq!(first.slug, "same-name");
    assert_eq!(second.slug, "same-name-2");
    assert_eq!(
        third.slug, "same-name-3",
        "the suffix keeps counting rather than retrying -2"
    );

    // Distinct ids as well as distinct slugs: these are three rows.
    assert_ne!(first.id, second.id);
    assert_eq!(
        list(&pool).await?.len(),
        3,
        "all three should exist; only the slug is deduplicated"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_rejects_explicit_duplicate_slug(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    create(
        &pool,
        CreateProject {
            name: "One".to_owned(),
            slug: Some("taken".to_owned()),
            ..CreateProject::default()
        },
    )
    .await?;

    let error = create(
        &pool,
        CreateProject {
            name: "Two".to_owned(),
            slug: Some("taken".to_owned()),
            ..CreateProject::default()
        },
    )
    .await
    .expect_err("an explicit slug that is taken must be refused");

    // Silently renaming it to "taken-2" would be worse: the caller asked for a
    // specific slug and would not know they got a different one.
    assert!(
        matches!(error, ProjectError::DuplicateSlug(_)),
        "got {error:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// list / get
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_returns_all_projects(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let a = create(&pool, input("Alpha Project")).await?;
    let b = create(&pool, input("Beta Project")).await?;

    let projects = list(&pool).await?;
    assert_eq!(projects.len(), 2);

    let ids: Vec<&str> = projects.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.contains(&a.id.as_str()));
    assert!(ids.contains(&b.id.as_str()));

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_filters_by_status(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let active = create(&pool, input("Alpha Project")).await?;
    create(&pool, input("Beta Project")).await?;
    update_status(&pool, &active.id, "active").await?;

    let filtered = list_by_status(&pool, "active").await?;
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, active.id);

    assert_eq!(
        list_by_status(&pool, "recon").await?.len(),
        1,
        "the other project should still be in recon"
    );
    assert_eq!(list_by_status(&pool, "archived").await?.len(), 0);

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_rejects_unknown_status_filter(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let error = list_by_status(&pool, "nonsense")
        .await
        .expect_err("an unknown status is not a filter, it is a mistake");
    assert!(
        matches!(error, ProjectError::InvalidStatus(_)),
        "got {error:?}"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn get_resolves_by_id(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let created = create(&pool, input("Alpha Project")).await?;

    let found = get(&pool, &created.id)
        .await?
        .expect("the row should exist");
    assert_eq!(found.id, created.id);
    assert_eq!(found.slug, created.slug);

    let by_slug = find_by_slug(&pool, &created.slug)
        .await?
        .expect("the slug should resolve too");
    assert_eq!(by_slug.id, created.id);

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn get_returns_none_for_missing(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    assert!(get(&pool, "does-not-exist").await?.is_none());
    assert!(find_by_slug(&pool, "no-such-slug").await?.is_none());

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn resolve_finds_by_id_or_slug(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let created = create(&pool, input("Alpha Project")).await?;

    // The id path is taken when the argument is 36 characters.
    let by_id = resolve(&pool, &created.id)
        .await?
        .expect("a 36-character id should resolve");
    assert_eq!(by_id.id, created.id);

    // Anything else is tried as a slug first.
    let by_slug = resolve(&pool, &created.slug)
        .await?
        .expect("a slug should resolve");
    assert_eq!(by_slug.id, created.id);

    assert!(
        resolve(&pool, "no-such-thing").await?.is_none(),
        "an unknown value resolves to nothing rather than erroring"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn resolve_id_returns_the_id_behind_a_slug(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let created = create(&pool, input("Alpha Project")).await?;

    // The same two paths as `resolve`, but reduced to the one value a caller
    // that stores ids needs.
    assert_eq!(
        resolve_id(&pool, &created.id).await?.as_deref(),
        Some(created.id.as_str())
    );
    assert_eq!(
        resolve_id(&pool, &created.slug).await?.as_deref(),
        Some(created.id.as_str())
    );

    // A name that is neither resolves to nothing, so the caller can turn it
    // into a "not found" rather than passing it on to a query that will
    // find nothing anyway.
    assert_eq!(resolve_id(&pool, "no-such-thing").await?, None);

    Ok(())
}

// ---------------------------------------------------------------------------
// event log
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_writes_event_log(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let project = create(&pool, input("Alpha Project")).await?;

    let rows =
        sqlx::query("SELECT entity_type, entity_id, action, actor, payload_json FROM event_log")
            .fetch_all(&pool)
            .await?;

    assert_eq!(rows.len(), 1, "one mutation, one event");
    assert_eq!(rows[0].get::<String, _>("entity_type"), "project");
    assert_eq!(rows[0].get::<String, _>("entity_id"), project.id);
    assert_eq!(rows[0].get::<String, _>("action"), "created");
    assert_eq!(rows[0].get::<String, _>("actor"), "user");

    let payload = rows[0]
        .get::<Option<String>, _>("payload_json")
        .expect("create should record a payload");
    assert!(
        payload.contains("Alpha Project") && payload.contains("alpha-project"),
        "the payload should name the project and its slug, got {payload}"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn update_status_writes_event_log(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let project = create(&pool, input("Alpha Project")).await?;
    update_status(&pool, &project.id, "active").await?;

    assert_eq!(event_count(&pool, &project.id, "created").await?, 1);
    assert_eq!(event_count(&pool, &project.id, "updated").await?, 1);

    let payload: Option<String> = sqlx::query_scalar(
        "SELECT payload_json FROM event_log WHERE entity_id = ? AND action = 'updated'",
    )
    .bind(&project.id)
    .fetch_one(&pool)
    .await?;

    let payload = payload.expect("the update should record a payload");
    assert!(
        payload.contains("recon") && payload.contains("active"),
        "the payload should record both ends of the transition, got {payload}"
    );

    let reloaded = get(&pool, &project.id).await?.expect("still there");
    assert_eq!(reloaded.status, "active");

    Ok(())
}
