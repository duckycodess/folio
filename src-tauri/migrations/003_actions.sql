-- Approved actions and recoverable history. Additive to 001/002.
ALTER TABLE action_plans ADD COLUMN status_message TEXT;
ALTER TABLE action_plans ADD COLUMN applied_at TEXT;

ALTER TABLE history ADD COLUMN operation_index INTEGER NOT NULL DEFAULT 0;
ALTER TABLE history ADD COLUMN operation_kind TEXT NOT NULL DEFAULT 'edit'
  CHECK(operation_kind IN ('edit','rename','move','create'));
ALTER TABLE history ADD COLUMN before_hash TEXT;
-- Recoverable content is kept for recent plans only; pruned entries can no longer be undone.
ALTER TABLE history ADD COLUMN content_pruned INTEGER NOT NULL DEFAULT 0;
CREATE INDEX history_plan_idx ON history(plan_id);
CREATE INDEX action_plans_workspace_idx ON action_plans(workspace_id, status);
