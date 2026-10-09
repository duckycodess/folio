-- Durable outcomes and recoverable history for the native writer. Additive to 001/002.
-- Plans and approvals stay in the native registry; a row here records a plan the
-- writer ran, without file bodies (`plan_json` holds a summary and the Ripple evidence).
ALTER TABLE action_plans ADD COLUMN applied_at TEXT;
ALTER TABLE action_plans ADD COLUMN stop_reason TEXT;

ALTER TABLE history ADD COLUMN operation_index INTEGER NOT NULL DEFAULT 0;
ALTER TABLE history ADD COLUMN operation_kind TEXT NOT NULL DEFAULT 'edit'
  CHECK(operation_kind IN ('edit','rename','move','create'));
ALTER TABLE history ADD COLUMN before_hash TEXT;
-- The document identity at the time of the change. Not a foreign key: a rename gives
-- the document a new identity, and history must keep naming the one that changed.
ALTER TABLE history ADD COLUMN document_ref TEXT;
-- 0 once an edit's previous content has been pruned; such an entry can no longer be undone.
ALTER TABLE history ADD COLUMN recoverable INTEGER NOT NULL DEFAULT 1;
CREATE INDEX history_plan_idx ON history(plan_id);
CREATE INDEX action_plans_applied_idx ON action_plans(workspace_id, applied_at);
