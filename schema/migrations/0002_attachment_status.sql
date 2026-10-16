-- no-transaction
-- This line MUST be the literal first bytes of the file. Any comment above
-- it makes sqlx silently ignore the directive. Verified against
-- sqlx-core-0.8.6/src/migrate/source.rs:127.
--
-- On SQLite this directive is currently INERT, and there is deliberately no
-- BEGIN/COMMIT below. sqlx-sqlite's Migrate::apply opens a transaction
-- unconditionally and never reads migration.no_tx, so every SQLite migration
-- is wrapped whether or not this directive is present. An explicit BEGIN
-- therefore fails outright with "cannot start a transaction within a
-- transaction". That wrapper is what makes this rebuild atomic, and it is
-- also why the PRAGMA statements below are no-ops on SQLite today: SQLite
-- ignores PRAGMA foreign_keys while a transaction is active. They are kept
-- because they are correct for backends that do honour the directive --
-- sqlx-postgres branches on no_tx -- and would take effect here if
-- sqlx-sqlite ever did the same.

PRAGMA foreign_keys = OFF;

CREATE TABLE attachment_new (
  id TEXT PRIMARY KEY,
  owner_type TEXT NOT NULL,
  owner_id TEXT NOT NULL,
  filename TEXT,
  mime TEXT,
  size_bytes INTEGER,
  status TEXT NOT NULL DEFAULT 'present',
  hash TEXT,
  blob_path TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  CHECK (
    (status = 'present' AND hash IS NOT NULL AND blob_path IS NOT NULL)
    OR (status != 'present')
  )
);

INSERT INTO attachment_new
  (id, owner_type, owner_id, filename, mime, size_bytes, status, hash, blob_path, created_at)
SELECT
  id, owner_type, owner_id, filename, mime, size_bytes,
  CASE
    WHEN hash IS NOT NULL AND blob_path IS NOT NULL THEN 'present'
    ELSE 'missing'
  END,
  hash, blob_path, created_at
FROM attachment;

DROP TABLE attachment;
ALTER TABLE attachment_new RENAME TO attachment;

CREATE INDEX idx_attachment_owner ON attachment(owner_type, owner_id);
CREATE INDEX idx_attachment_hash ON attachment(hash);

PRAGMA foreign_key_check;

PRAGMA foreign_keys = ON;
