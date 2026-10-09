-- Recoverable deletion (issue #44, ADR 0010). SQLite cannot change a CHECK constraint,
-- so `history` is rebuilt with the same columns, constraints and defaults as 001 + 003,
-- except that `operation_kind` also allows 'delete'. Every existing row is kept. No
-- table references `history`, so nothing else has to be rebuilt.
CREATE TABLE history_new (
  id TEXT PRIMARY KEY,
  plan_id TEXT NOT NULL REFERENCES action_plans(id),
  document_id TEXT REFERENCES documents(id),
  before_path TEXT,
  after_path TEXT,
  before_content BLOB,
  after_hash TEXT,
  applied_at TEXT NOT NULL,
  undone_at TEXT,
  operation_index INTEGER NOT NULL DEFAULT 0,
  operation_kind TEXT NOT NULL DEFAULT 'edit'
    CHECK(operation_kind IN ('edit','rename','move','create','delete')),
  before_hash TEXT,
  -- The document identity at the time of the change. Not a foreign key: a rename gives
  -- the document a new identity, and history must keep naming the one that changed.
  document_ref TEXT,
  -- 0 once the content an edit or a deletion needs has been pruned; such an entry can no
  -- longer be undone. A deletion has no after path or hash.
  recoverable INTEGER NOT NULL DEFAULT 1,
  -- Unix permission bits of a deleted file, so Undo restores a private file as private.
  before_mode INTEGER
);
INSERT INTO history_new (id, plan_id, document_id, before_path, after_path, before_content, after_hash, applied_at, undone_at, operation_index, operation_kind, before_hash, document_ref, recoverable)
  SELECT id, plan_id, document_id, before_path, after_path, before_content, after_hash, applied_at, undone_at, operation_index, operation_kind, before_hash, document_ref, recoverable FROM history;
DROP TABLE history;
ALTER TABLE history_new RENAME TO history;
CREATE INDEX history_plan_idx ON history(plan_id);
