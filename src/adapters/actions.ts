import { invoke } from "@tauri-apps/api/core";
import type {
  ApplyResult,
  FileOperation,
  FilenameSuggestion,
  HistoryEntry,
  OrganizeSuggestions,
  PlanPreview,
  UndoResult,
} from "../domain/contracts";

/**
 * Native plan → approve → apply → undo. Operations must come from the user's request
 * (directly or through the provider's interpretation), never from document text.
 * Creating a preview writes nothing; only `applyPlan` on an approved plan changes files.
 */
export function createPlan(
  workspaceId: string,
  operations: FileOperation[],
): Promise<PlanPreview> {
  return invoke<PlanPreview>("create_plan", { workspaceId, operations });
}

/** Approves exactly the previewed plan; the digest must be the one shown in the preview. */
export function approvePlan(
  workspaceId: string,
  plan: Pick<PlanPreview, "id" | "digest">,
): Promise<PlanPreview> {
  return invoke<PlanPreview>("approve_plan", {
    workspaceId,
    planId: plan.id,
    digest: plan.digest,
  });
}

/** Rejects with a NativeError when nothing was written; resolves `failed` after a partial write. */
export function applyPlan(
  workspaceId: string,
  planId: string,
): Promise<ApplyResult> {
  return invoke<ApplyResult>("apply_plan", { workspaceId, planId });
}

/** Reverses a whole applied plan, or rejects with UNDO_CONFLICT without touching files. */
export function undoPlan(
  workspaceId: string,
  planId: string,
): Promise<UndoResult> {
  return invoke<UndoResult>("undo_plan", { workspaceId, planId });
}

export function listHistory(workspaceId: string): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("list_history", { workspaceId });
}

export function organizeSuggestions(
  workspaceId: string,
): Promise<OrganizeSuggestions> {
  return invoke<OrganizeSuggestions>("organize_suggestions", { workspaceId });
}

/** The rename operation for a chosen filename suggestion; preview it with `createPlan`. */
export function renameOperation(suggestion: FilenameSuggestion): FileOperation {
  return {
    kind: "rename",
    documentId: suggestion.documentId,
    expectedContentHash: suggestion.contentHash,
    destinationRelativePath: suggestion.suggestedRelativePath,
  };
}
