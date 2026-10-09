-- Virtual collections (#78, ADR 0013): named groups of document references.
-- Keeping or editing one changes no file, so it is not an action plan.
CREATE TABLE collections (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX collections_workspace_idx ON collections(workspace_id);

-- A member follows Folio's own renames and moves. While Folio has deleted its
-- file, `removed_by_history_id` names that deletion; undoing it clears the mark.
CREATE TABLE collection_members (
  collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  document_id TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  added_at TEXT NOT NULL,
  removed_by_history_id TEXT,
  PRIMARY KEY (collection_id, document_id)
);
CREATE INDEX collection_members_document_idx ON collection_members(document_id);
CREATE INDEX collection_members_removed_idx ON collection_members(removed_by_history_id);
