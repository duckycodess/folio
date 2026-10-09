-- Index lifecycle state. Additive to 001; offsets in chunks are UTF-16 code units
-- into the document's extracted text (see src/domain/contracts.ts).
ALTER TABLE workspaces ADD COLUMN last_opened_at TEXT;
CREATE UNIQUE INDEX workspaces_root_idx ON workspaces(root_path);

ALTER TABLE documents ADD COLUMN name TEXT NOT NULL DEFAULT '';
ALTER TABLE documents ADD COLUMN title TEXT;
ALTER TABLE documents ADD COLUMN status TEXT NOT NULL DEFAULT 'indexed'
  CHECK(status IN ('indexed','unsupported','failed','stale'));
ALTER TABLE documents ADD COLUMN status_message TEXT;
CREATE INDEX documents_hash_idx ON documents(workspace_id, content_hash);

-- Generated summaries and other per-document caches; invalidated when the document changes.
CREATE TABLE derived_cache (
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  payload TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(document_id, kind)
);
