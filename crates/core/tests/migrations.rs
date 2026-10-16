//! Migration tests.
//!
//! `schema/migrations` is the source of truth for the schema. Most migrations
//! are ordinary `ALTER TABLE` statements, but 0002 is the first that cannot
//! be: SQLite has no way to relax a `NOT NULL` or add a `CHECK` constraint, so
//! it rebuilds the table instead.
//!
//! Each database test uses `#[sqlx::test]`, which gives every test its own
//! throwaway database, so no test can see another's rows and none needs
//! cleanup. `migrations = false` hands back a bare database when a test needs
//! to control which migrations run — which is how the 0002 tests apply 0001
//! first and then 0002 on top of real rows.

use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};

const MIGRATIONS_DIR: &str = "../../schema/migrations";
const M1: &str = "0001_init.sql";
const M2: &str = "0002_attachment_status.sql";

fn migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(MIGRATIONS_DIR)
}

fn migration_path(name: &str) -> PathBuf {
    migrations_dir().join(name)
}

fn migration_sql(name: &str) -> String {
    std::fs::read_to_string(migration_path(name))
        .unwrap_or_else(|error| panic!("{name} should be readable: {error}"))
}

/// `name` and `sql` for every object in the database, ordered by name.
async fn schema_snapshot(pool: &SqlitePool) -> Result<Vec<(String, String)>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT name, COALESCE(sql, '') AS sql FROM sqlite_master ORDER BY name")
            .fetch_all(pool)
            .await?;

    Ok(rows
        .into_iter()
        .map(|row| (row.get::<String, _>("name"), row.get::<String, _>("sql")))
        .collect())
}

/// (found, is_not_null, default) for one column.
async fn describe_column(
    pool: &SqlitePool,
    table: &str,
    column: &str,
) -> Result<(bool, bool, String), sqlx::Error> {
    let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(pool)
        .await?;

    Ok(rows
        .iter()
        .find(|row| row.get::<String, _>("name") == column)
        .map(|row| {
            (
                true,
                row.get::<i64, _>("notnull") == 1,
                row.get::<Option<String>, _>("dflt_value")
                    .unwrap_or_default(),
            )
        })
        .unwrap_or((false, false, String::new())))
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn fresh_database_schema_matches_expected(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await?;
    assert_eq!(versions, vec![1, 2, 3], "all three should be recorded");

    // Read the shape back from the database rather than from the SQL that was
    // written; the point is what SQLite actually built.
    let (found, not_null, default) = describe_column(&pool, "attachment", "status").await?;
    assert!(found, "attachment.status should exist");
    assert!(not_null, "status should be NOT NULL");
    assert!(
        default.contains("present"),
        "status should default to 'present', got {default:?}"
    );

    let (found, not_null, _) = describe_column(&pool, "attachment", "hash").await?;
    assert!(found);
    assert!(
        !not_null,
        "hash must be nullable, or a referenced-but-uncaptured attachment cannot be recorded"
    );

    let (found, not_null, _) = describe_column(&pool, "attachment", "blob_path").await?;
    assert!(found);
    assert!(!not_null, "blob_path must be nullable for the same reason");

    let create_sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'attachment'",
    )
    .fetch_one(&pool)
    .await?;
    assert!(
        create_sql.contains("CHECK"),
        "the rebuilt table should carry the CHECK constraint, got: {create_sql}"
    );

    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'attachment'",
    )
    .fetch_all(&pool)
    .await?;
    for expected in ["idx_attachment_owner", "idx_attachment_hash"] {
        assert!(
            indexes.iter().any(|name| name == expected),
            "{expected} should exist after the rebuild; found {indexes:?}"
        );
    }

    Ok(())
}

#[sqlx::test(migrations = false)]
async fn migration_0002_preserves_existing_rows(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Apply 0001 by hand so that 0002 runs on top of real rows. A
    // fresh-database test cannot reach the copy in the middle of a rebuild.
    sqlx::raw_sql(&migration_sql(M1)).execute(&pool).await?;

    sqlx::query(
        "INSERT INTO attachment
           (id, owner_type, owner_id, filename, mime, size_bytes, hash, blob_path)
         VALUES
           ('a1', 'message', 'm1', 'notes.txt', 'text/plain', 12, 'deadbeef', '/tmp/notes.txt')",
    )
    .execute(&pool)
    .await?;

    sqlx::raw_sql(&migration_sql(M2)).execute(&pool).await?;

    let row =
        sqlx::query("SELECT hash, blob_path, status, filename FROM attachment WHERE id = 'a1'")
            .fetch_one(&pool)
            .await?;

    assert_eq!(
        row.get::<String, _>("status"),
        "present",
        "a row with both hash and blob_path should migrate as present"
    );
    assert_eq!(row.get::<String, _>("hash"), "deadbeef");
    assert_eq!(row.get::<String, _>("blob_path"), "/tmp/notes.txt");
    assert_eq!(row.get::<String, _>("filename"), "notes.txt");

    Ok(())
}

#[sqlx::test(migrations = false)]
async fn migration_0002_marks_incomplete_rows_missing(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    // DEVIATION FROM THE BRIEF, deliberate and reported.
    //
    // The brief calls for inserting a row with a NULL blob_path and asserting
    // it migrates to 'missing'. That row cannot exist: 0001 declares
    // `hash TEXT NOT NULL, blob_path TEXT NOT NULL` and SQLite enforces it, so
    // the ELSE branch of 0002's CASE is unreachable from any database created
    // by 0001.
    //
    // What is worth pinning is that fact, because it is load-bearing for how
    // 0002 should be read: the ELSE branch is defensive, not a behaviour
    // anyone can currently observe. If v1's NOT NULLs are ever relaxed, this
    // test fails and whoever relaxed them learns the branch just became live.
    sqlx::raw_sql(&migration_sql(M1)).execute(&pool).await?;

    let incomplete = sqlx::query(
        "INSERT INTO attachment (id, owner_type, owner_id, hash, blob_path)
         VALUES ('a-inc', 'message', 'm1', 'deadbeef', NULL)",
    )
    .execute(&pool)
    .await;

    assert!(
        incomplete.is_err(),
        "v1 should reject a NULL blob_path. If this insert now succeeds, the NOT NULL was \
         relaxed and 0002's ELSE branch is reachable, so this test must be rewritten to assert \
         status='missing' rather than to assert the insert fails."
    );

    // Confirm the reachable half of the mapping still holds.
    sqlx::query(
        "INSERT INTO attachment (id, owner_type, owner_id, hash, blob_path)
         VALUES ('a-ok', 'message', 'm1', 'cafebabe', '/tmp/a')",
    )
    .execute(&pool)
    .await?;

    sqlx::raw_sql(&migration_sql(M2)).execute(&pool).await?;

    let status: String = sqlx::query_scalar("SELECT status FROM attachment WHERE id = 'a-ok'")
        .fetch_one(&pool)
        .await?;
    assert_eq!(status, "present");

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn migration_0002_check_rejects_incomplete_present(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    // A CHECK constraint that is never exercised is a comment, not a
    // constraint.
    let outcome = sqlx::query(
        "INSERT INTO attachment (id, owner_type, owner_id, status, hash, blob_path)
         VALUES ('a1', 'message', 'm1', 'present', NULL, '/tmp/x')",
    )
    .execute(&pool)
    .await;

    assert!(
        outcome.is_err(),
        "an attachment claiming status 'present' with no hash must be rejected"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn migration_0002_check_allows_missing_with_nulls(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    // The point of the migration: before it, an attachment that was referenced
    // but never captured could not be represented at all.
    sqlx::query(
        "INSERT INTO attachment (id, owner_type, owner_id, status, hash, blob_path)
         VALUES ('a1', 'message', 'm1', 'missing', NULL, NULL)",
    )
    .execute(&pool)
    .await?;

    let status: String = sqlx::query_scalar("SELECT status FROM attachment WHERE id = 'a1'")
        .fetch_one(&pool)
        .await?;

    assert_eq!(status, "missing");

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn migration_0002_foreign_keys_enforced_after(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    // DEVIATION FROM THE BRIEF, deliberate and reported.
    //
    // The brief describes inserting "an attachment with a non-existent
    // owner_id in a child table that references it". No such reference exists:
    // attachment keys on a generic owner_type/owner_id, and nothing in 0001
    // declares a foreign key to it. Enforcement is therefore asserted against
    // a reference that does exist, message.conversation_id -> conversation(id).
    //
    // Note what this does and does not prove. `PRAGMA foreign_keys` is
    // connection-scoped, and sqlx enables it per connection by default, so a
    // fresh pool would enforce foreign keys whether or not 0002 ends with the
    // pragma. The migration's trailing `PRAGMA foreign_keys = ON` only
    // restores the setting on the connection the migration ran on. Both facts
    // are worth knowing before treating this test as proof about 0002 itself.
    let setting: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await?;
    assert_eq!(setting, 1, "foreign key enforcement should be on");

    let outcome = sqlx::query(
        "INSERT INTO message (id, conversation_id, role, content_json)
         VALUES ('m1', 'no-such-conversation', 'user', '{}')",
    )
    .execute(&pool)
    .await;

    assert!(
        outcome.is_err(),
        "a message referencing a missing conversation must be rejected while enforcement is on"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn migration_0003_adds_content_hash(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query("INSERT INTO source (id, name) VALUES ('s1', 'chatgpt')")
        .execute(&pool)
        .await?;

    sqlx::query(
        "INSERT INTO conversation (id, source_id, title, content_hash)
         VALUES ('c1', 's1', 'a conversation', 'hash-value')",
    )
    .execute(&pool)
    .await?;

    let hash: Option<String> =
        sqlx::query_scalar("SELECT content_hash FROM conversation WHERE id = 'c1'")
            .fetch_one(&pool)
            .await?;

    assert_eq!(
        hash.as_deref(),
        Some("hash-value"),
        "content_hash should round-trip"
    );

    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'conversation'",
    )
    .fetch_all(&pool)
    .await?;
    assert!(
        indexes.iter().any(|name| name == "idx_conversation_hash"),
        "idx_conversation_hash should exist; found {indexes:?}"
    );

    Ok(())
}

#[sqlx::test(migrations = false)]
async fn all_migrations_idempotent(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let migrator = sqlx::migrate::Migrator::new(migrations_dir()).await?;

    migrator.run(&pool).await?;
    let before = schema_snapshot(&pool).await?;

    migrator.run(&pool).await?; // must be a no-op, not an error
    let after = schema_snapshot(&pool).await?;

    assert_eq!(before, after, "a second run must not change the schema");

    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(applied, 3, "a second run must not re-apply anything");

    Ok(())
}

/// Guards the bytes that sqlx's migration loader looks for.
///
/// sqlx reads the literal start of the file
/// (`sqlx-core-0.8.6/src/migrate/source.rs:127`):
///
/// ```text
/// let no_tx = sql.starts_with("-- no-transaction");
/// ```
///
/// A BOM, a blank line, or any comment above the directive makes that false.
/// So this test exists to catch a future contributor prepending a license
/// header to 0002.
///
/// Note what is *not* part of that contract: the newline. An earlier version
/// of this test required `-- no-transaction\n` and so failed on Windows, where
/// `core.autocrlf` checks the file out with CRLF. Line endings are now pinned
/// to LF for `schema/migrations/*.sql` in `.gitattributes`, because sqlx
/// checksums a migration as `Sha384` over its raw bytes
/// (`sqlx-core-0.8.6/src/migrate/migration.rs:25`) and compares that checksum
/// on every run (`migrator.rs:175`, `MigrateError::VersionMismatch`). Without
/// the attribute a CRLF checkout checksums differently from an LF one, so a
/// database written by one is rejected by the other.
///
/// Be precise about what the flag does on SQLite, because it is not what the
/// directive's name suggests: it is parsed and stored, and then never read.
/// `sqlx-sqlite`'s `Migrate::apply` opens a transaction unconditionally, and
/// sqlx-core's `run_direct` calls `apply` with no branch on `migration.no_tx`.
/// `sqlx-postgres` does branch on it. So on SQLite every migration is wrapped
/// whether or not the directive is present — which is why 0002 contains no
/// `BEGIN`, since an explicit one fails with "cannot start a transaction
/// within a transaction".
///
/// The bytes are still worth pinning: they are the contract with sqlx's
/// loader, they matter on backends that honour the flag, and they would start
/// mattering here if sqlx-sqlite ever consulted it.
///
/// No database is involved, so this one is a plain `#[test]`.
#[test]
fn no_transaction_directive_is_first_bytes() {
    let bytes = std::fs::read(migration_path(M2)).expect("0002 should be readable");

    // sqlx's contract is `sql.starts_with("-- no-transaction")`: 17 bytes, no
    // newline. Pinning the newline as well made this a line-ending test by
    // accident.
    const DIRECTIVE: &[u8] = b"-- no-transaction";

    assert!(
        !bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
        "0002 must not begin with a UTF-8 BOM; sqlx compares literal bytes"
    );

    assert!(
        bytes.starts_with(DIRECTIVE),
        "0002 must begin with the directive. Got: {:?}",
        String::from_utf8_lossy(&bytes[..bytes.len().min(64)])
    );

    // The directive is a line of its own, so whatever follows it terminates
    // that line. Accepting CR as well as LF is what makes this test pass on a
    // Windows checkout; it still rejects `-- no-transactionish`.
    assert!(
        matches!(bytes.get(DIRECTIVE.len()), Some(b'\n' | b'\r')),
        "the directive must be a line of its own. Got: {:?}",
        String::from_utf8_lossy(&bytes[..bytes.len().min(64)])
    );
}
