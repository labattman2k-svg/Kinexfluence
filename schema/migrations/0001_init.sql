-- Kinexfluence schema v1. See Step 1 of the build plan.

CREATE TABLE source (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  display_name TEXT,
  adapter_version TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE conversation (
  id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL REFERENCES source(id),
  external_id TEXT,
  title TEXT,
  url TEXT,
  model TEXT,
  created_at DATETIME,
  updated_at DATETIME,
  imported_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  raw_path TEXT,
  metadata_json TEXT
);
CREATE INDEX idx_conversation_source ON conversation(source_id, external_id);
CREATE INDEX idx_conversation_updated ON conversation(updated_at);

CREATE TABLE message (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
  parent_id TEXT REFERENCES message(id),
  branch_id TEXT,
  is_active_branch INTEGER NOT NULL DEFAULT 1,
  edit_of TEXT REFERENCES message(id),
  role TEXT NOT NULL,
  turn_index INTEGER,
  created_at DATETIME,
  content_json TEXT NOT NULL,
  token_count INTEGER,
  raw_json TEXT
);
CREATE INDEX idx_message_conversation ON message(conversation_id, turn_index);

CREATE TABLE snippet (
  id TEXT PRIMARY KEY,
  message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  block_index INTEGER NOT NULL,
  segment_index INTEGER NOT NULL DEFAULT 0,
  granularity TEXT NOT NULL DEFAULT 'block',
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  language TEXT,
  char_start INTEGER,
  char_end INTEGER,
  hash TEXT NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_snippet_message ON snippet(message_id);
CREATE INDEX idx_snippet_hash ON snippet(hash);

CREATE TABLE project (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  slug TEXT UNIQUE,
  description TEXT,
  brief TEXT,
  status TEXT NOT NULL DEFAULT 'recon',
  weights_json TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE project_document (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  filename TEXT,
  mime TEXT,
  size_bytes INTEGER,
  path TEXT NOT NULL,
  extracted_text_path TEXT,
  embedding_id TEXT,
  added_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE item (
  id TEXT PRIMARY KEY,
  snippet_id TEXT REFERENCES snippet(id),
  kind TEXT NOT NULL,
  title TEXT,
  text TEXT NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  metadata_json TEXT
);

CREATE TABLE item_project (
  item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  status TEXT NOT NULL DEFAULT 'cart',
  relevance_score REAL,
  auto_selected INTEGER NOT NULL DEFAULT 0,
  user_forced INTEGER NOT NULL DEFAULT 0,
  added_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  committed_at DATETIME,
  PRIMARY KEY (item_id, project_id)
);
CREATE INDEX idx_item_project_status ON item_project(project_id, status);

CREATE TABLE tag (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  color TEXT
);

CREATE TABLE item_tag (
  item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  tag_id TEXT NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
  PRIMARY KEY (item_id, tag_id)
);

CREATE TABLE item_link (
  id TEXT PRIMARY KEY,
  from_item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  to_item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  relation TEXT NOT NULL,
  confidence REAL,
  created_by TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE probe (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  trigger TEXT NOT NULL,
  questions_json TEXT NOT NULL,
  answers_json TEXT NOT NULL,
  intent TEXT,
  rationale TEXT,
  impact TEXT,
  confidence TEXT,
  status TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE embedding (
  id TEXT PRIMARY KEY,
  owner_type TEXT NOT NULL,
  owner_id TEXT NOT NULL,
  model TEXT NOT NULL,
  vector BLOB NOT NULL,
  dim INTEGER NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_embedding_owner ON embedding(owner_type, owner_id);

CREATE TABLE attachment (
  id TEXT PRIMARY KEY,
  owner_type TEXT NOT NULL,
  owner_id TEXT NOT NULL,
  filename TEXT,
  mime TEXT,
  size_bytes INTEGER,
  hash TEXT NOT NULL,
  blob_path TEXT NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_attachment_owner ON attachment(owner_type, owner_id);
CREATE INDEX idx_attachment_hash ON attachment(hash);

CREATE TABLE context_pack (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  version INTEGER NOT NULL,
  format TEXT NOT NULL,
  path TEXT,
  item_ids_json TEXT NOT NULL,
  notes TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE event_log (
  id TEXT PRIMARY KEY,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  action TEXT NOT NULL,
  actor TEXT NOT NULL,
  payload_json TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_event_log_entity ON event_log(entity_type, entity_id);
