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
  | "applied"
  | "partlyUndone"
  | "undone"
  | "stopped"
  | "nothingChanged"
  | "unknown";

/**
 * One approved plan Folio ran, as the Activity timeline shows it. The native
 * listing carries each operation's outcome; the batch's kind, status and
 * counts are derived here, deterministically, so they never disagree.
 */
export interface ActivityEntry {
  planId: string;
  /** When Folio started running it, in epoch milliseconds. */
  appliedAt: number;
  source: PlanSource;
  /** Every operation in order, including any that failed or never ran. */
  operations: ActivityOperation[];
  /** Changes with recorded history and their current Undo state. */
  entries: HistoryEntry[];
  kinds: ChangeKind[];
  /** Files that changed, with or without recoverable history. */
  changed: number;
  /** Operations from before outcomes were stored, with no known outcome. */
  unrecorded: number;
  status: BatchStatus;
  stopReason?: BatchStopReason;
  /** True when some change can still be reversed; Undo's preview has the final say. */
  canUndo: boolean;
}

/** A recorded write whose history is unavailable; it still changed its file. */
export function changedWithoutHistory(operation: ActivityOperation): boolean {
  return (
    !operation.history &&
    (operation.status === "succeeded" ||
      (operation.status === "failed" &&
        operation.error?.code === "historyRequired"))
  );
}

function changedCount(operations: ActivityOperation[]): number {
  return operations.filter(
    (operation) => operation.history || changedWithoutHistory(operation),
  ).length;
}

function unrecordedCount(operations: ActivityOperation[]): number {
  return operations.filter(
    (operation) => !operation.history && operation.status === undefined,
  ).length;
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
export function fromActivity(recorded: RecordedBatch[]): ActivityEntry[] {
  return recorded.map((batch) => {
    const entries = batch.operations.flatMap((operation) =>
      operation.history ? [operation.history] : [],
    );
    const undone = entries.filter((entry) => entry.undoneAt !== undefined);
    const stopped =
      batch.stopReason === "failed" || batch.stopReason === "cancelled";
    const changed = changedCount(batch.operations);
    const unrecorded = unrecordedCount(batch.operations);
    const status: BatchStatus =
      unrecorded > 0
        ? "unknown"
        : changed === 0
          ? "nothingChanged"
          : undone.length === changed
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
      changed,
      unrecorded,
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

function files(count: number): string {
  return `${count} ${count === 1 ? "file" : "files"}`;
}

/**
 * "Moved 3 files", "Changed 4 files" for a mix, "Moved 1 of 3 files" when it
 * stopped partway, and "Couldn't move 1 file" when nothing changed.
 */
export function batchTitle(batch: ActivityEntry): string {
  const single = batch.kinds.length === 1 ? batch.kinds[0] : null;
  const total = batch.operations.length;
  const changed = batch.changed;
  if (batch.status === "unknown")
    return `Attempted to ${single ?? "change"} ${files(total)}`;
  if (changed === 0) return `Couldn't ${single ?? "change"} ${files(total)}`;
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
  unknown: "Outcome not fully recorded",
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
export function stopSummary(batch: ActivityEntry): string | null {
  const total = batch.operations.length;
  const { changed, unrecorded } = batch;
  const unknownNotice =
    unrecorded > 0
      ? `Folio didn't record what happened to ${files(unrecorded)} in this change.`
      : null;
  const failed = batch.operations.find(
    (operation) => operation.status === "failed",
  );
  if (batch.stopReason === "failed" && failed) {
    const reason = failed.error?.message ?? "Folio couldn't make this change.";
    const kept =
      batch.entries.length > 0
        ? ` The ${batch.entries.length === 1 ? "earlier change was" : "earlier changes were"} kept.`
        : changed === 0 && unrecorded === 0
          ? " Nothing was changed."
          : "";
    const undo = changedWithoutHistory(failed)
      ? " Undo isn't available for this file."
      : "";
    return `Stopped at ${operationPath(failed)}: ${reason}${kept}${undo}${unknownNotice ? ` ${unknownNotice}` : ""}`;
  }
  if (unknownNotice) return unknownNotice;
  if (batch.stopReason === "cancelled")
    return `Cancelled after ${changed} of ${total} changes. The rest weren't started.`;
  return null;
}

/**
 * One page of Activity from a listing asked for one batch more than `size`:
 * the extra batch only says that older ones exist.
 */
export function activityPage(
  recorded: RecordedBatch[],
  size: number,
): { batches: ActivityEntry[]; hasOlder: boolean } {
  return {
    batches: fromActivity(recorded.slice(0, size)),
    hasOlder: recorded.length > size,
  };
}

/** Why Undo can't reverse a file, in words. */
export const CONFLICT_REASONS: Record<UndoConflictReason, string> = {
  externallyModified: "was changed after Folio's change",
  missing: "is missing",
  destinationOccupied: "can't go back: another file now uses its earlier name",
  notRecoverable: "can't go back: its earlier version wasn't kept",
};
