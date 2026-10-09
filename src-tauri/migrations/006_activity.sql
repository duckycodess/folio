-- Activity (#35): where each applied plan was started, and every operation's
-- outcome, so failed and cancelled batches can be shown, not only what changed.
-- Additive to 001–005. Plans recorded earlier keep `source = 'unknown'` and no
-- outcomes: Activity rebuilds them from their history and never guesses the rest.
ALTER TABLE action_plans ADD COLUMN source TEXT NOT NULL DEFAULT 'unknown'
  CHECK(source IN ('home','organize','graph','assistant','summary','unknown'));
-- The batch result as JSON: each operation's status, error and history entry id.
ALTER TABLE action_plans ADD COLUMN outcome_json TEXT;
