import type { HistoryEntry, UndoConflictReason } from "./contracts";

/** What one operation did, from its recorded before and after state. */
export type ChangeKind = "rename" | "move" | "edit" | "create" | "delete";

export type BatchStatus = "applied" | "partlyUndone" | "undone";

/** One approved plan's recorded changes, as the Activity timeline shows them. */
export interface ActivityBatch {
  planId: string;
  /** When the first operation was applied, in epoch milliseconds. */
  appliedAt: number;
  entries: HistoryEntry[];
  kinds: ChangeKind[];
  status: BatchStatus;
  /** True when some change can still be reversed; Undo's preview has the final say. */
  canUndo: boolean;
}

function folder(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash === -1 ? "" : path.slice(0, slash);
}

export function changeKind(entry: HistoryEntry): ChangeKind {
  const before = entry.beforeRelativePath;
  const after = entry.afterRelativePath;
  if (before === undefined) return "create";
  if (after === undefined) return "delete";
  if (before === after) return "edit";
  return folder(before) === folder(after) ? "rename" : "move";
}

/**
 * Groups history rows into one batch per plan, newest first. Only rows the
 * native history recorded appear, so previews and analyses never do.
 */
export function groupHistory(entries: HistoryEntry[]): ActivityBatch[] {
  const byPlan = new Map<string, HistoryEntry[]>();
  for (const entry of entries) {
    const rows = byPlan.get(entry.planId) ?? [];
    rows.push(entry);
    byPlan.set(entry.planId, rows);
  }
  return [...byPlan.entries()]
    .map(([planId, rows]) => {
      const sorted = [...rows].sort(
        (a, b) => a.operationIndex - b.operationIndex,
      );
      const undone = sorted.filter((entry) => entry.undoneAt !== undefined);
      return {
        planId,
        appliedAt: Math.min(...sorted.map((entry) => entry.appliedAt)),
        entries: sorted,
        kinds: [...new Set(sorted.map(changeKind))],
        status:
          undone.length === 0
            ? "applied"
            : undone.length === sorted.length
              ? "undone"
              : "partlyUndone",
        canUndo: sorted.some(
          (entry) => entry.recoverable && entry.undoneAt === undefined,
        ),
      } satisfies ActivityBatch;
    })
    .sort((a, b) => b.appliedAt - a.appliedAt);
}

const VERBS: Record<ChangeKind, string> = {
  rename: "Renamed",
  move: "Moved",
  edit: "Edited",
  create: "Created",
  delete: "Deleted",
};

/** "Moved 3 files", "Renamed 1 file", or "Changed 4 files" for a mix. */
export function batchTitle(batch: ActivityBatch): string {
  const count = batch.entries.length;
  const files = `${count} ${count === 1 ? "file" : "files"}`;
  return batch.kinds.length === 1
    ? `${VERBS[batch.kinds[0]]} ${files}`
    : `Changed ${files}`;
}

export const STATUS_LABELS: Record<BatchStatus, string> = {
  applied: "Applied",
  partlyUndone: "Partly undone",
  undone: "Undone",
};

/** Why Undo can't reverse a file, in words. */
export const CONFLICT_REASONS: Record<UndoConflictReason, string> = {
  externallyModified: "was changed after Folio's change",
  missing: "is missing",
  destinationOccupied: "can't go back: another file now uses its earlier name",
  notRecoverable: "can't go back: its earlier version wasn't kept",
};
