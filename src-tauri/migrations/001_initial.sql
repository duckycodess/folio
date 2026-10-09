PRAGMA foreign_keys = ON;

CREATE TABLE workspaces (id TEXT PRIMARY KEY, root_path TEXT NOT NULL, authorized_at TEXT NOT NULL);
CREATE TABLE documents (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  relative_path TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  media_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0),
  modified_at TEXT NOT NULL,
  indexed_at TEXT,
  UNIQUE(workspace_id, relative_path)
);
CREATE TABLE chunks (
  chunk_id INTEGER PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  chunk_text TEXT NOT NULL,
  start_offset INTEGER NOT NULL,
  end_offset INTEGER NOT NULL,
  page INTEGER,
  content_hash TEXT NOT NULL,
  UNIQUE(document_id, ordinal)
);
CREATE VIRTUAL TABLE chunks_fts USING fts5(chunk_text, content='chunks', content_rowid='chunk_id');
CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
  INSERT INTO chunks_fts(rowid, chunk_text) VALUES (new.chunk_id, new.chunk_text);
END;
CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
  INSERT INTO chunks_fts(chunks_fts, rowid, chunk_text) VALUES ('delete', old.chunk_id, old.chunk_text);
END;
CREATE TRIGGER chunks_au AFTER UPDATE ON chunks BEGIN
  INSERT INTO chunks_fts(chunks_fts, rowid, chunk_text) VALUES ('delete', old.chunk_id, old.chunk_text);
  INSERT INTO chunks_fts(rowid, chunk_text) VALUES (new.chunk_id, new.chunk_text);
END;
CREATE TABLE embedding_spaces (
  id TEXT PRIMARY KEY,
  model_id TEXT NOT NULL,
  revision TEXT NOT NULL,
  quantization TEXT NOT NULL,
  dimensions INTEGER NOT NULL CHECK(dimensions > 0),
  preprocessing_fingerprint TEXT NOT NULL,
  UNIQUE(model_id, revision, quantization, dimensions, preprocessing_fingerprint)
);
CREATE TABLE embeddings (
  chunk_id INTEGER NOT NULL REFERENCES chunks(chunk_id) ON DELETE CASCADE,
  space_id TEXT NOT NULL REFERENCES embedding_spaces(id) ON DELETE CASCADE,
  vector BLOB NOT NULL,
  PRIMARY KEY(chunk_id, space_id)
);
CREATE TABLE relationships (
  id TEXT PRIMARY KEY,
  source_document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  target_document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  relationship_type TEXT NOT NULL CHECK(relationship_type IN ('similarity','explicitReference','sharedFactCandidate')),
  evidence_json TEXT NOT NULL,
  provenance TEXT NOT NULL,
  confidence REAL CHECK(confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
  source_content_hash TEXT NOT NULL,
  target_content_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE action_plans (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  plan_json TEXT NOT NULL,
  plan_digest TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('preview','approved','applied','expired','failed')),
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);
CREATE TABLE approvals (plan_id TEXT PRIMARY KEY REFERENCES action_plans(id), plan_digest TEXT NOT NULL, approved_at TEXT NOT NULL);
CREATE TABLE history (
  id TEXT PRIMARY KEY,
  plan_id TEXT NOT NULL REFERENCES action_plans(id),
  document_id TEXT REFERENCES documents(id),
  before_path TEXT,
  after_path TEXT,
  before_content BLOB,
  after_hash TEXT,
  applied_at TEXT NOT NULL,
  undone_at TEXT
);
CREATE TABLE benchmark_results (id TEXT PRIMARY KEY, case_id TEXT NOT NULL, task_type TEXT NOT NULL, model_id TEXT NOT NULL, conditions_json TEXT NOT NULL, measurements_json TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE INDEX chunks_document_idx ON chunks(document_id);
CREATE INDEX relationships_source_idx ON relationships(source_document_id);
CREATE INDEX relationships_target_idx ON relationships(target_document_id);
