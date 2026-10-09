import { invoke } from "@tauri-apps/api/core";
import type {
  ActionPlan,
  ApplyReport,
  Approval,
  DocumentId,
  FileOperation,
  HistoryEntry,
  ImpactCandidate,
  OrganizationSuggestions,
  UndoPreflight,
  UndoReport,
  WorkspaceId,
} from "../domain/contracts";
import { toFolioError } from "../domain/errors";

/**
 * The action boundary. The native core issues plan identities and digests,
 * records approvals and owns every write; this module only carries requests
 * across. The UI cannot mint a plan or an approval the native core will accept.
 */
async function call<T>(command: string, args?: Record<string, unknown>) {
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    throw toFolioError(cause);
  }
}

/**
 * Prepares an exact, expiring plan. Nothing is written. Without `impacts`,
 * the native core computes Ripple candidates from each edit's diff.
 */
export function preparePlan(
  workspaceId: WorkspaceId,
  operations: FileOperation[],
  impacts?: ImpactCandidate[],
): Promise<ActionPlan> {
  return call<ActionPlan>("prepare_plan", {
    workspaceId,
    operations,
    impacts,
  });
}

/** Approve exactly the plan that was shown, by echoing its digest. */
export function approvePlan(
  workspaceId: WorkspaceId,
  plan: ActionPlan,
): Promise<Approval> {
  return call<Approval>("approve_plan", {
    workspaceId,
    planId: plan.id,
    planDigest: plan.digest,
  });
}

/**
 * Applies an approved plan. Rejects with a `FolioError` when nothing was
 * written; otherwise the report holds one durable outcome per operation. A
 * failure stops the batch and keeps earlier changes, which Undo can reverse.
 */
export function applyPlan(
  workspaceId: WorkspaceId,
  plan: ActionPlan,
): Promise<ApplyReport> {
  return call<ApplyReport>("apply_plan", { workspaceId, planId: plan.id });
}

/** Stops a running apply before its next operation; finished changes are kept. */
export function cancelApply(): Promise<void> {
  return call<void>("cancel_apply");
}

/** What Undo would reverse and anything blocking it. Writes nothing. */
export function previewUndo(
  workspaceId: WorkspaceId,
  planId: string,
): Promise<UndoPreflight> {
  return call<UndoPreflight>("preview_undo", { workspaceId, planId });
}

/**
 * Reverses the entries of the preview the user confirmed. If anything changed
 * since that preview, nothing is undone. A partial Undo leaves the rest
 * pending; preview again to finish it.
 */
export function undoPlan(
  workspaceId: WorkspaceId,
  preview: UndoPreflight,
): Promise<UndoReport> {
  return call<UndoReport>("undo_plan", {
    workspaceId,
    planId: preview.planId,
    entryIds: preview.entryIds,
  });
}

/** The most recent history entries, newest plan first (at most 500). */
export function listHistory(
  workspaceId: WorkspaceId,
  limit = 100,
): Promise<HistoryEntry[]> {
  return call<HistoryEntry[]>("list_history", { workspaceId, limit });
}

/** Ripple candidates for an explicit phrase, e.g. the value an interpreter replaced. */
export function rippleImpacts(
  workspaceId: WorkspaceId,
  documentId: DocumentId,
  replacedText: string,
): Promise<ImpactCandidate[]> {
  return call<ImpactCandidate[]>("ripple_impacts", {
    workspaceId,
    documentId,
    replacedText,
  });
}

/**
 * The edit operation that replaces one exact passage. Rejects with
 * `operationUnsupported` (`details.reason` is `passageNotFound` or
 * `passageAmbiguous`) when the passage does not occur exactly once.
 */
export function preparePassageEdit(
  workspaceId: WorkspaceId,
  documentId: DocumentId,
  before: string,
  after: string,
): Promise<FileOperation> {
  return call<FileOperation>("prepare_passage_edit", {
    workspaceId,
    documentId,
    before,
    after,
  });
}

/** Duplicate groups (evidence only) and filename suggestions with their rename operations. */
export function organizationSuggestions(
  workspaceId: WorkspaceId,
): Promise<OrganizationSuggestions> {
  return call<OrganizationSuggestions>("organization_suggestions", {
    workspaceId,
  });
}
