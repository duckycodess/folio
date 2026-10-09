import {
  EDITABLE_MEDIA_TYPES,
  type ActionPlan,
  type Approval,
  type BatchResult,
  type BatchStopReason,
  type ContentHash,
  type FolioErrorPayload,
  type HistoryEntry,
  type OperationOutcome,
  type RelativePath,
  type UndoConflict,
  type UndoPreflight,
} from "./contracts";
import { folioError } from "./errors";
import { hashBytes, isContentHash } from "./hash";
import {
  assertPortableDestination,
  mediaTypeForPath,
  normalizeRelativePath,
} from "./identity";

const CANONICAL_HEADER = "FOLIO-PLAN-V1";
const encoder = new TextEncoder();

function field(value: string): string {
  return `${encoder.encode(value).length}:${value}\n`;
}

/**
 * Canonical bytes of a plan. Every field is length-prefixed in UTF-8 bytes, so
 * no path or document body can forge a field boundary and two plans that differ
 * anywhere produce different bytes.
 *
 * The digest covers exactly what can change a file: plan identity, workspace,
 * the validity window and every operation in order. `impacts` are review
 * candidates that never write, so they are excluded; adding or removing a
 * Ripple candidate does not silently invalidate an approval.
 */
export function canonicalPlanBytes(plan: ActionPlan): Uint8Array {
  let text = `${CANONICAL_HEADER}\n`;
  text += field(plan.id);
  text += field(plan.workspaceId);
  text += field(String(plan.createdAt));
  text += field(String(plan.expiresAt));
  text += field(String(plan.operations.length));
  for (const operation of plan.operations) {
    text += field(operation.kind);
    if (operation.kind === "create") {
      text += field(operation.destinationRelativePath);
      text += field(operation.mediaType);
      text += field(operation.content);
    } else if (operation.kind === "edit") {
      text += field(operation.documentId);
      text += field(operation.relativePath);
      text += field(operation.expectedContentHash);
      text += field(operation.after);
    } else {
      text += field(operation.documentId);
      text += field(operation.relativePath);
      text += field(operation.expectedContentHash);
      text += field(operation.destinationRelativePath);
    }
  }
  return encoder.encode(text);
}

/** `sha256` over the canonical bytes. The native core is authoritative. */
export async function planDigest(plan: ActionPlan): Promise<ContentHash> {
  return hashBytes(canonicalPlanBytes(plan));
}

/**
 * Recompute the digest and refuse a plan whose declared digest does not match
 * its own operations — a plan edited after the native core issued it.
 */
export async function verifyPlanDigest(plan: ActionPlan): Promise<void> {
  if (!isContentHash(plan.digest)) {
    throw folioError("planDigestMismatch", "This plan has no usable digest.", {
      planId: plan.id,
    });
  }
  const recomputed = await planDigest(plan);
  if (recomputed !== plan.digest) {
    throw folioError(
      "planDigestMismatch",
      "This plan changed after it was prepared. Review a fresh preview.",
      { planId: plan.id, expected: plan.digest, observed: recomputed },
    );
  }
}

/** What the filesystem currently shows for one path. */
export interface ObservedPath {
  exists: boolean;
  /** Hash of the current bytes; null when the path is absent or unreadable. */
  contentHash?: ContentHash | null;
  /** True when the path exists and is a regular file. */
  isFile?: boolean;
}

export type ObservedPaths = Record<RelativePath, ObservedPath>;

/** Every workspace path an operation reads from or writes to. */
export function planPaths(plan: ActionPlan): RelativePath[] {
  const paths: RelativePath[] = [];
  for (const operation of plan.operations) {
    if (operation.kind === "create") {
      paths.push(operation.destinationRelativePath);
      continue;
    }
    paths.push(operation.relativePath);
    if (operation.kind !== "edit")
      paths.push(operation.destinationRelativePath);
  }
  return paths;
}

function observe(observed: ObservedPaths, path: RelativePath): ObservedPath {
  const entry = observed[path];
  if (!entry) {
    throw folioError(
      "internal",
      "Preflight needs the current state of every path in the plan.",
      { path },
    );
  }
  return entry;
}

/**
 * The key two paths share when the filesystem would treat them as one file.
 * Windows and macOS default to case-insensitive names, so a batch that renames
 * `Plan.md` and edits `plan.md` is acting on the same file twice.
 */
function targetKey(path: RelativePath): string {
  return path.toLowerCase();
}

function sameTarget(left: RelativePath, right: RelativePath): boolean {
  return targetKey(left) === targetKey(right);
}

function assertEditable(path: RelativePath): void {
  const mediaType = mediaTypeForPath(path);
  if (!mediaType || !EDITABLE_MEDIA_TYPES.includes(mediaType as never)) {
    throw folioError(
      "unsupportedMediaType",
      "Folio edits TXT and Markdown files. Text-based PDFs are read-only.",
      { path },
    );
  }
}

/**
 * Check every operation in a batch before any file changes, as the accepted
 * batch-failure policy requires. Throws the first typed failure; a plan that
 * passes preflight is the only thing a writer may start.
 */
export function preflightPlan(
  plan: ActionPlan,
  observed: ObservedPaths,
  now: number,
): void {
  if (plan.operations.length === 0) {
    throw folioError("planEmpty", "This plan contains no operations.", {
      planId: plan.id,
    });
  }
  if (now < plan.createdAt || now >= plan.expiresAt) {
    throw folioError(
      "planExpired",
      "This preview is no longer current. Review a fresh preview.",
      { planId: plan.id },
    );
  }
  // Two passes. The whole batch is checked structurally first, so a plan that
  // can never be valid is refused the same way whatever the current files
  // happen to be, and only then is it compared against observed state.
  //
  // Windows and macOS folders are usually case-insensitive, so two operations
  // naming the same file in different cases are the same file in practice.
  const touched = new Map<string, RelativePath>();
  const checked: {
    source?: RelativePath;
    destination?: RelativePath;
  }[] = [];
  for (const operation of plan.operations) {
    let source: RelativePath | undefined;
    let destination: RelativePath | undefined;

    if (operation.kind !== "create") {
      source = normalizeRelativePath(operation.relativePath);
      assertEditable(source);
    }
    if (operation.kind !== "edit") {
      destination = assertPortableDestination(
        operation.destinationRelativePath,
      );
      assertEditable(destination);
      if (source !== undefined && sameTarget(destination, source)) {
        throw folioError(
          "operationUnsupported",
          "A rename needs a destination different from the current name.",
          { path: source },
        );
      }
    }

    for (const path of [source, destination]) {
      if (path === undefined) continue;
      const earlier = touched.get(targetKey(path));
      if (earlier !== undefined) {
        throw folioError(
          "duplicateOperationTarget",
          "Two operations in this plan act on the same file.",
          { path, earlierPath: earlier },
        );
      }
      touched.set(targetKey(path), path);
    }
    checked.push({ source, destination });
  }

  for (const [index, operation] of plan.operations.entries()) {
    const { source, destination } = checked[index];
    if (source !== undefined && operation.kind !== "create") {
      const current = observe(observed, source);
      if (!current.exists) {
        throw folioError(
          "targetMissing",
          "The file this plan changes is no longer there.",
          { path: source },
        );
      }
      if (current.isFile === false) {
        throw folioError(
          "operationUnsupported",
          "That name is not a file Folio can change.",
          { path: source },
        );
      }
      if (current.contentHash !== operation.expectedContentHash) {
        throw folioError(
          "targetChanged",
          "This file changed since the preview was prepared. Review a fresh preview.",
          {
            path: source,
            expected: operation.expectedContentHash,
            observed: current.contentHash ?? "absent",
          },
        );
      }
    }

    if (destination !== undefined && observe(observed, destination).exists) {
      throw folioError(
        "destinationExists",
        "Something already uses that name. The existing file was left alone.",
        { path: destination },
      );
    }
  }
}

export function assertApprovalMatches(
  plan: ActionPlan,
  approval: Approval,
): void {
  if (approval.planId !== plan.id) {
    throw folioError(
      "approvalStale",
      "This approval belongs to a different preview.",
      { planId: plan.id, approvedPlanId: approval.planId },
    );
  }
  if (approval.planDigest !== plan.digest) {
    throw folioError(
      "approvalStale",
      "The plan changed after it was approved. Review a fresh preview.",
      { planId: plan.id },
    );
  }
}

/** One operation's result reported by the writer, in application order. */
export type AttemptOutcome =
  | { status: "succeeded"; historyEntryId: string; completedAt: number }
  | { status: "failed"; error: FolioErrorPayload; completedAt: number };

export interface SettleBatchInput {
  plan: ActionPlan;
  approval: Approval;
  /** A prefix of `plan.operations`; the first failure must be the last entry. */
  attempts: AttemptOutcome[];
  /**
   * Index of the last operation allowed to finish after the user cancelled.
   * Operations after it are recorded as `cancelled`, never rolled back.
   */
  cancelledAfterIndex?: number;
  startedAt: number;
  finishedAt: number;
}

/**
 * Turn reported attempts into the durable per-operation record.
 *
 * Stop on the first failure; everything after it is `notStarted`. On
 * cancellation the running operation finishes and everything after it is
 * `cancelled`. A failure in the final attempt wins over a pending cancellation,
 * because the failure is what stopped the batch.
 */
export function settleBatch(input: SettleBatchInput): BatchResult {
  const { plan, approval, attempts } = input;
  assertApprovalMatches(plan, approval);
  if (attempts.length > plan.operations.length) {
    throw folioError(
      "planStateInvalid",
      "More operations were reported than this plan contains.",
      { planId: plan.id },
    );
  }
  const failureIndex = attempts.findIndex(
    (attempt) => attempt.status === "failed",
  );
  if (failureIndex >= 0 && failureIndex !== attempts.length - 1) {
    throw folioError(
      "planStateInvalid",
      "A batch must stop at its first failed operation.",
      { planId: plan.id, operationIndex: String(failureIndex) },
    );
  }
  const cancelledAfterIndex = input.cancelledAfterIndex;
  if (cancelledAfterIndex !== undefined) {
    // A cancellation reported with no attempts would describe an operation that
    // never ran. Nothing began, so there is no outcome to record.
    if (attempts.length === 0 || cancelledAfterIndex !== attempts.length - 1) {
      throw folioError(
        "planStateInvalid",
        "Cancellation must stop after the operation that was already running.",
        { planId: plan.id, operationIndex: String(cancelledAfterIndex) },
      );
    }
  }

  const outcomes: OperationOutcome[] = attempts.map((attempt, index) => {
    if (attempt.status === "succeeded") {
      if (!attempt.historyEntryId.trim()) {
        throw folioError(
          "historyRequired",
          "A completed operation must have a recoverable history entry.",
          { planId: plan.id, operationIndex: String(index) },
        );
      }
      return {
        operationIndex: index,
        status: "succeeded",
        completedAt: attempt.completedAt,
        historyEntryId: attempt.historyEntryId,
      };
    }
    return {
      operationIndex: index,
      status: "failed",
      completedAt: attempt.completedAt,
      error: attempt.error,
    };
  });

  let stopReason: BatchStopReason;
  if (failureIndex >= 0) stopReason = "failed";
  else if (
    cancelledAfterIndex !== undefined &&
    attempts.length < plan.operations.length
  )
    stopReason = "cancelled";
  else if (attempts.length === plan.operations.length) stopReason = "completed";
  else {
    throw folioError(
      "planStateInvalid",
      "A batch stopped early without a failure or a cancellation.",
      { planId: plan.id },
    );
  }

  const remainingStatus =
    stopReason === "cancelled" ? "cancelled" : "notStarted";
  for (
    let index = attempts.length;
    index < plan.operations.length;
    index += 1
  ) {
    outcomes.push({ operationIndex: index, status: remainingStatus });
  }

  return {
    planId: plan.id,
    planDigest: plan.digest,
    startedAt: input.startedAt,
    finishedAt: input.finishedAt,
    outcomes,
    stopReason,
  };
}

/** History entries for the operations that actually changed a file. */
export function succeededOutcomes(result: BatchResult): OperationOutcome[] {
  return result.outcomes.filter((outcome) => outcome.status === "succeeded");
}

/**
 * Whole-batch Undo preflight. Every applied entry is checked against the
 * current file state first; if anything conflicts, no file is touched and the
 * blocking file is named, so newer external edits survive.
 */
export function preflightUndo(input: {
  planId: string;
  entries: HistoryEntry[];
  observed: ObservedPaths;
}): UndoPreflight {
  const conflicts: UndoConflict[] = [];
  const entries = input.entries.filter((entry) => entry.undoneAt === undefined);
  for (const entry of entries) {
    const appliedPath = entry.afterRelativePath ?? entry.beforeRelativePath;
    if (!appliedPath) {
      conflicts.push({
        historyEntryId: entry.id,
        documentId: entry.documentId,
        relativePath: "",
        observedContentHash: null,
        reason: "notRecoverable",
      });
      continue;
    }
    if (!entry.recoverable) {
      conflicts.push({
        historyEntryId: entry.id,
        documentId: entry.documentId,
        relativePath: appliedPath,
        observedContentHash: null,
        reason: "notRecoverable",
      });
      continue;
    }
    const current = observe(input.observed, appliedPath);
    if (!current.exists) {
      conflicts.push({
        historyEntryId: entry.id,
        documentId: entry.documentId,
        relativePath: appliedPath,
        expectedContentHash: entry.afterContentHash,
        observedContentHash: null,
        reason: "missing",
      });
      continue;
    }
    if (current.contentHash !== entry.afterContentHash) {
      conflicts.push({
        historyEntryId: entry.id,
        documentId: entry.documentId,
        relativePath: appliedPath,
        expectedContentHash: entry.afterContentHash,
        observedContentHash: current.contentHash ?? null,
        reason: "externallyModified",
      });
      continue;
    }
    const restoredPath = entry.beforeRelativePath;
    if (restoredPath && restoredPath !== appliedPath) {
      const destination = observe(input.observed, restoredPath);
      if (destination.exists) {
        conflicts.push({
          historyEntryId: entry.id,
          documentId: entry.documentId,
          relativePath: restoredPath,
          observedContentHash: destination.contentHash ?? null,
          reason: "destinationOccupied",
        });
      }
    }
  }
  return {
    planId: input.planId,
    entryIds: entries.map((entry) => entry.id),
    conflicts,
    undoable: conflicts.length === 0,
  };
}

/** Refuse a whole-batch Undo when any entry conflicts, naming the blocker. */
export function assertUndoable(preflight: UndoPreflight): void {
  if (preflight.undoable) return;
  const blocking = preflight.conflicts[0];
  throw folioError(
    "undoConflict",
    "One of these files changed after Folio saved it, so nothing was undone.",
    {
      planId: preflight.planId,
      blockingRelativePath: blocking.relativePath,
      blockingHistoryEntryId: blocking.historyEntryId,
      reason: blocking.reason,
      conflicts: String(preflight.conflicts.length),
    },
  );
}

/** Paths an Undo preflight needs to observe. */
export function undoPaths(entries: HistoryEntry[]): RelativePath[] {
  const paths: RelativePath[] = [];
  for (const entry of entries) {
    if (entry.undoneAt !== undefined) continue;
    const applied = entry.afterRelativePath ?? entry.beforeRelativePath;
    if (applied) paths.push(applied);
    if (entry.beforeRelativePath && entry.beforeRelativePath !== applied) {
      paths.push(entry.beforeRelativePath);
    }
  }
  return paths;
}
