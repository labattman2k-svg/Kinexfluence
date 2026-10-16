ALTER TABLE conversation ADD COLUMN content_hash TEXT;
CREATE INDEX idx_conversation_hash ON conversation(content_hash);
