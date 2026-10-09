-- Virtual collections (#78, ADR 0016): named groups of document references.
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
-- Only visible members are unique, so a hidden row never blocks, or is changed
-- by, a new file that takes the same identity before the deletion is undone.
CREATE TABLE collection_members (
  id INTEGER PRIMARY KEY,
  collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  document_id TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  added_at TEXT NOT NULL,
  removed_by_history_id TEXT
);
CREATE UNIQUE INDEX collection_members_visible_idx ON collection_members(collection_id, document_id) WHERE removed_by_history_id IS NULL;
CREATE INDEX collection_members_document_idx ON collection_members(document_id);
CREATE INDEX collection_members_removed_idx ON collection_members(removed_by_history_id);
