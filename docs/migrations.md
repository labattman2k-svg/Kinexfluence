# Migration patterns

`schema/migrations` is the source of truth for the database schema. This
guide records the rules a migration must follow, and the failure modes that
are specific to SQLite and sqlx — particularly the ones that do not produce
an error.

## 1. Forward only

- Migrations are numbered and ordered. A migration that has been merged is
  **never edited** (`docs/STANDARDS.md` §2).
- sqlx stores a checksum of every applied migration. Editing a merged file
  changes its checksum, and every existing database then refuses to open.
  That failure is at least loud.
- There is no down migration; schema downgrade is not supported (§4). To undo
  a change, write a new forward migration.
- To change a column's shape, add a new column and backfill, or rebuild the
  table (§3).

## 2. sqlx wraps each migration in a transaction

sqlx runs each migration inside a transaction, so a failed migration leaves
nothing behind. That is the behaviour you want most of the time.

It is also why `PRAGMA foreign_keys = OFF` often appears to do nothing.
SQLite treats that pragma as a **no-op while a transaction is active**. Set
it inside sqlx's wrapping transaction and it is silently ignored, the DDL
proceeds with foreign keys enforced, and nothing warns you.

### The `-- no-transaction` directive, and why it does nothing on SQLite

The directive exists. A migration can opt out of the wrapper by beginning
with the literal text:

```sql
-- no-transaction
```

sqlx reads the **first bytes of the file** to decide
(`sqlx-core-0.8.6/src/migrate/source.rs:127`):

```rust
let no_tx = sql.starts_with("-- no-transaction");
```

That is a `starts_with` on raw text, so each of these makes it false:

- a UTF-8 BOM
- a blank line
- a license header
- any comment above the directive — including a comment explaining the
  directive

**But on SQLite the flag is then ignored.** This was verified by reading the
crates rather than the documentation, and it is worth stating plainly because
it inverts the usual advice:

- `sqlx-core` computes `no_tx` and stores it on the `Migration`.
- `sqlx-sqlite`'s `Migrate::apply` opens a transaction **unconditionally**
  and never reads `migration.no_tx`. Nothing in `sqlx-sqlite` references the
  field at all.
- `sqlx-core`'s `run_direct` calls `conn.apply(migration)` with no branch on
  the flag.
- `sqlx-postgres`, by contrast, *does* branch on it
  (`sqlx-postgres-0.8.6/src/migrate.rs:214,258`).

So on SQLite **every migration is wrapped, always**, and two consequences
follow:

1. **`PRAGMA foreign_keys = OFF` in a SQLite migration never takes effect.**
   It is inside the wrapper, so SQLite ignores it. The rebuild in 0002 works
   despite this only because nothing in `0001_init.sql` references
   `attachment` by foreign key.
2. **An explicit `BEGIN`/`COMMIT` in a SQLite migration fails.** It produces
   `cannot start a transaction within a transaction`. 0002 therefore contains
   no `BEGIN`, even though the general 12-step procedure calls for one. The
   wrapper sqlx already provides is what makes that rebuild atomic.

If SQLite's migrator ever starts consulting the flag, 0002's pragmas become
live and a `BEGIN` would become correct. The directive is kept for that
reason, and for backends that honour it today.

### How to verify the bytes

`crates/core/tests/migrations.rs::no_transaction_directive_is_first_bytes`
reads the file as **bytes** and asserts that it starts with
`-- no-transaction\n` and carries no BOM.

Byte-level rather than line-level, because a BOM or a stray blank line is
invisible in a diff and in most editors. If a later migration needs the
directive, add the equivalent assertion for that file to the same test. Do
not rely on review to catch it.

## 3. Rebuilding a table (SQLite's 12-step pattern)

SQLite's `ALTER TABLE` supports exactly four operations: `RENAME TO`,
`ADD COLUMN`, `DROP COLUMN` (3.35+), and `RENAME COLUMN`. That is the whole
list.

It cannot:

- drop a `NOT NULL` constraint from an existing column
- add a `CHECK` constraint
- change a column's type
- add a foreign key to an existing table

Anything on that list means rebuilding the table.
`schema/migrations/0002_attachment_status.sql` is the first migration here
that had to, and is the reference example:

1. `PRAGMA foreign_keys = OFF` — **outside** any transaction
2. `BEGIN`
3. create the replacement table with the desired shape
4. `INSERT INTO new SELECT ... FROM old`, mapping values as needed
5. `DROP TABLE old`
6. `ALTER TABLE new RENAME TO old`
7. recreate every index the old table had
8. recreate triggers and views that referenced it
9. `PRAGMA foreign_key_check` — abort if the copied data violates a constraint
10. `COMMIT`
11. `PRAGMA foreign_keys = ON`

Steps 1, 2, 10 and 11 are the parts that do not survive contact with SQLite:
none of them can take effect there, because sqlx has already opened a
transaction and will reject the `BEGIN`. See §2. Write the rebuild steps
(3–9) and let sqlx supply the atomicity.

Two details that are easy to miss:

- **Recreate the indexes.** Indexes belong to the table that was dropped and
  go with it. Nothing errors when they are missing; queries just quietly get
  slower, which is the worst kind of regression to find later.
- **`ALTER TABLE ... RENAME` rewrites references** to the renamed table in
  other tables' foreign keys, views, and triggers, from SQLite 3.25 onward.
  Usually helpful, but it means a rename during a rebuild can edit objects
  you did not mean to touch. Check what references the table first.

Nothing in `0001_init.sql` references `attachment` by foreign key, which is
what makes 0002 safe today. That is not a coincidence worth relying on, so
the actual behaviour was measured. Rebuilding a table that another table
references, run through the real migrator, gives:

| State of the referencing table | Result |
|---|---|
| Empty | **succeeds**, and the referencing FK survives the rename intact |
| Holds a row pointing at the rebuilt table | **fails**: `FOREIGN KEY constraint failed` (SQLite code 787) |

The reason is the interaction described in §2. Because `PRAGMA foreign_keys =
OFF` is inert under the migrator, foreign keys stay **enforced** through the
rebuild, and `DROP TABLE` performs an implicit delete of the parent rows,
which violates any child row that references them. The pragma would have
prevented exactly this, and it is precisely the pragma that does not work.

The data dependence is the dangerous part. A rebuild like this passes against
an empty development database and fails on a user's populated one. If a
future migration has to rebuild a table that anything references by foreign
key, stop and ask: the pattern here is not sufficient. A custom migration
runner that can genuinely disable foreign keys, or an explicit
drop-FK / rebuild / restore-FK sequence, is required.

### `PRAGMA foreign_key_check` is only a guard if you read its result

Step 9 above is usually written as a bare statement:

```sql
PRAGMA foreign_key_check;
```

That returns rows — one per violation — and does not itself abort anything.
Executed as a bare statement inside a migration, its result is discarded, so
it documents intent and nothing more. Making it an actual guard means reading
the result and rolling back when it is non-empty, which a plain `.sql`
migration cannot do. Treat it as a comment that happens to be executable.

## 4. What breaks silently

| Symptom | Cause | Fails loudly? |
|---|---|---|
| `PRAGMA foreign_keys` has no effect | SQLite ignores it inside a transaction, and on SQLite every migration is in one | no |
| `-- no-transaction` was ignored | on SQLite it always is; elsewhere, something precedes it in the file | no |
| Queries slower after a rebuild | indexes were not recreated | no |
| Data quietly reshaped | the rebuild's `CASE`/`SELECT` mapping normalized values | no |
| A `foreign_key_check` guard that never guards | its result was discarded | no |
| Existing databases refuse to open | a merged migration file was edited | yes — checksum |
| Migration aborts mid-rebuild | sqlx's wrapping transaction rolled back | yes |

The silently-failing rows are the reason this document exists. Each one leaves
a database that works, so nothing surfaces until the consequences do.

On atomicity: because sqlx wraps every SQLite migration whether or not the
directive is present, a rebuild is atomic for free. A failure after
`DROP TABLE` rolls back and the table is still there. Do **not** add an
explicit `BEGIN`/`COMMIT` to a SQLite migration to "make it safe" — it will
fail outright. That guarantee comes from the wrapper, not from the file.

## 5. Testing a migration

Three kinds of test, all in `crates/core/tests/migrations.rs`. Every database
test uses `#[sqlx::test]`, which gives each test its own throwaway database,
so tests cannot see each other's rows and need no cleanup.

**Schema shape.** `PRAGMA table_info(table)` returns `cid`, `name`, `type`,
`notnull`, `dflt_value`, `pk`. Assert on `notnull` and `dflt_value` rather
than trusting the SQL you wrote — the point is to check what the database
actually built.

**Constraint behaviour.** Insert a row that should be rejected and assert
that the insert fails. A `CHECK` constraint that is never exercised is a
comment, not a constraint.

**A rebuild against real data.** This is the part a fresh-database test
cannot reach: whether the copy in the middle of a rebuild actually carried
the rows across. Apply the migrations before the target, insert rows in the
old shape, apply the target, and assert the rows survived:

```rust
#[sqlx::test(migrations = false)]
async fn rebuild_preserves_rows(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(&migration_sql("0001_init.sql")).execute(&pool).await?;
    // insert rows in the pre-migration shape
    sqlx::raw_sql(&migration_sql("0002_attachment_status.sql")).execute(&pool).await?;
    // assert the rows survived, with the values the mapping was meant to produce
    Ok(())
}
```

`migrations = false` hands you a bare database when a test needs to control
which migrations run. `raw_sql` executes a whole migration file, including
files that contain multiple statements and pragmas.

## 6. Checklist for a new migration

- [ ] New file with the next number; never an edit to an existing one
- [ ] `-- no-transaction` only if a pragma or an explicit transaction is
      needed — and as the literal first bytes if so
- [ ] Indexes recreated if the table was rebuilt
- [ ] `PRAGMA foreign_key_check` before `COMMIT` in a rebuild
- [ ] Tested against a database that already holds rows, not only a fresh one
- [ ] `CHANGELOG.md` updated
- [ ] `docs/STANDARDS_EXCEPTIONS.md` entry if a hard gate was bent
