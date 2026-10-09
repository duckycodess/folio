# Activity records attempts, and each plan carries its source

Activity first listed only history rows, so it could show what changed but not what failed, what was cancelled, or which page started a change. Issue #35 asked for all three.

**Activity lists every approved plan Folio ran, not only successful changes.** The writer already kept one `action_plans` row per applied plan with its status and stop reason, but threw away the per-operation outcomes. They are now stored with that row, and `list_activity` returns one batch per plan with every operation's status, error and history. A plan that failed before changing anything is listed as "nothing was changed"; a plan that was prepared or approved but never applied is still not activity. `history` keeps only rows that changed a file, because Undo depends on it.

**Each plan carries its source, inside the digest.** The UI names where a plan was started (`home`, `organize`, `graph`, `assistant`, `summary`) when it asks for the plan. Including it in the canonical bytes (now `FOLIO-PLAN-V2`) means a plan can't be relabelled after approval, and a closed list means no document or model output can supply it. The cost is a frozen-contract change: both digest implementations and the golden fixtures moved together, for TJ's review.

**Nothing is guessed for older plans.** Plans recorded before this read as `unknown`; an operation with history succeeded, and one without has no status. Batch rows are small and are kept; only file contents are pruned, as before.

Decided by Gab on 2026-10-10 for issue #35.
