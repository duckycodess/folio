import type {
  ActivityBatch as RecordedBatch,
  ActivityOperation,
  BatchStopReason,
  FileOperationKind,
  HistoryEntry,
  PlanSource,
  UndoConflictReason,
} from "./contracts";

/** What one operation did, from its recorded before and after state. */
export type ChangeKind = FileOperationKind;

/**
 * `stopped`: a failure or a cancellation ended the batch after some changes.
 * `nothingChanged`: it was approved and run, but no file changed.
 */
export type BatchStatus =
  "applied" | "partlyUndone" | "undone" | "stopped" | "nothingChanged";

/** One approved plan Folio ran, as the Activity timeline shows it. */
export interface ActivityBatch {
  planId: string;
  /** When Folio started running it, in epoch milliseconds. */
  appliedAt: number;
  source: PlanSource;
  /** Every operation in order, including any that failed or never ran. */
  operations: ActivityOperation[];
  /** The changes that happened: one history entry per changed file. */
  entries: HistoryEntry[];
  kinds: ChangeKind[];
  status: BatchStatus;
  stopReason?: BatchStopReason;
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
 * Activity's batches from the native listing, in its order (newest first).
 * Only plans Folio ran appear, so previews and analyses never do. A failure
 * or cancellation is shown as it was recorded; nothing is guessed.
 */
export function fromActivity(recorded: RecordedBatch[]): ActivityBatch[] {
  return recorded.map((batch) => {
    const entries = batch.operations.flatMap((operation) =>
      operation.history ? [operation.history] : [],
    );
    const undone = entries.filter((entry) => entry.undoneAt !== undefined);
    const stopped =
      batch.stopReason === "failed" || batch.stopReason === "cancelled";
    const status: BatchStatus =
      entries.length === 0
        ? "nothingChanged"
        : undone.length === entries.length
          ? "undone"
          : undone.length > 0
            ? "partlyUndone"
            : stopped
              ? "stopped"
              : "applied";
    return {
      planId: batch.planId,
      appliedAt: batch.appliedAt,
      source: batch.source,
      operations: batch.operations,
      entries,
      kinds: [
        ...new Set(
          batch.operations.map((operation) => operation.operationKind),
        ),
      ],
      status,
      stopReason: batch.stopReason,
      canUndo: entries.some(
        (entry) => entry.recoverable && entry.undoneAt === undefined,
      ),
    };
  });
}

const VERBS: Record<ChangeKind, string> = {
  rename: "Renamed",
  move: "Moved",
  edit: "Edited",
  create: "Created",
  delete: "Deleted",
};

const ATTEMPTS: Record<ChangeKind, string> = {
  rename: "rename",
  move: "move",
  edit: "edit",
  create: "create",
  delete: "delete",
};

function files(count: number): string {
  return `${count} ${count === 1 ? "file" : "files"}`;
}

/**
 * "Moved 3 files", "Changed 4 files" for a mix, "Moved 1 of 3 files" when it
 * stopped partway, and "Couldn't move 1 file" when nothing changed.
 */
export function batchTitle(batch: ActivityBatch): string {
  const single = batch.kinds.length === 1 ? batch.kinds[0] : null;
  const total = batch.operations.length;
  const changed = batch.entries.length;
  if (changed === 0)
    return `Couldn't ${single ? ATTEMPTS[single] : "change"} ${files(total)}`;
  const verb = single ? VERBS[single] : "Changed";
  return changed < total
    ? `${verb} ${changed} of ${files(total)}`
    : `${verb} ${files(changed)}`;
}

export const STATUS_LABELS: Record<BatchStatus, string> = {
  applied: "Applied",
  partlyUndone: "Partly undone",
  undone: "Undone",
  stopped: "Stopped",
  nothingChanged: "Nothing changed",
};

/** Where the change was started, or `null` when Folio didn't record it. */
export const SOURCE_LABELS: Record<PlanSource, string | null> = {
  home: "From Home",
  organize: "From Organize",
  graph: "From Graph",
  assistant: "From Ask & Act",
  summary: "From a file's summary",
  unknown: null,
};

function operationPath(operation: ActivityOperation): string {
  return (
    operation.afterRelativePath ??
    operation.beforeRelativePath ??
    `change ${operation.operationIndex + 1}`
  );
}

/**
 * Why a batch ended early, in words, or `null` when it ran to the end. For
 * batches recorded before outcomes were kept, says what wasn't recorded.
 */
export function stopSummary(batch: ActivityBatch): string | null {
  const total = batch.operations.length;
  const changed = batch.entries.length;
  const failed = batch.operations.find(
    (operation) => operation.status === "failed",
  );
  if (batch.stopReason === "failed" && failed) {
    const reason = failed.error?.message ?? "Folio couldn't make this change.";
    const kept =
      changed > 0
        ? ` The ${changed === 1 ? "earlier change was" : "earlier changes were"} kept.`
        : " Nothing was changed.";
    return `Stopped at ${operationPath(failed)}: ${reason}${kept}`;
  }
  if (batch.stopReason === "cancelled")
    return `Cancelled after ${changed} of ${total} changes. The rest weren't started.`;
  const unrecorded = batch.operations.filter(
    (operation) => operation.status === undefined,
  ).length;
  if (unrecorded > 0)
    return `Folio didn't record what happened to ${files(unrecorded)} in this change.`;
  return null;
}

/** Why Undo can't reverse a file, in words. */
export const CONFLICT_REASONS: Record<UndoConflictReason, string> = {
  externallyModified: "was changed after Folio's change",
  missing: "is missing",
  destinationOccupied: "can't go back: another file now uses its earlier name",
  notRecoverable: "can't go back: its earlier version wasn't kept",
};
