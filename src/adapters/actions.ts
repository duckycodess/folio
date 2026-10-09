import { invoke } from "@tauri-apps/api/core";
import type {
  ActionPlan,
  Approval,
  FileOperation,
  ImpactCandidate,
  WorkspaceId,
} from "../domain/contracts";
import { toFolioError } from "../domain/errors";

/**
 * The action boundary. The native core issues plan identities and digests,
 * records approvals and owns every write; this module only carries requests
 * across. The UI cannot mint a plan or an approval the native core will accept.
 *
 * `applyPlan` currently rejects with `writerNotImplemented` until the native
 * writer lands in issue #5. Treat that as the honest state, not an error to
 * hide: no file has been changed.
 */
export async function preparePlan(
  workspaceId: WorkspaceId,
  operations: FileOperation[],
  impacts: ImpactCandidate[] = [],
): Promise<ActionPlan> {
  try {
    return await invoke<ActionPlan>("prepare_plan", {
      workspaceId,
      operations,
      impacts,
    });
  } catch (cause) {
    throw toFolioError(cause);
  }
}

/** Approve exactly the plan that was shown, by echoing its digest. */
export async function approvePlan(
  workspaceId: WorkspaceId,
  plan: ActionPlan,
): Promise<Approval> {
  try {
    return await invoke<Approval>("approve_plan", {
      workspaceId,
      planId: plan.id,
      planDigest: plan.digest,
    });
  } catch (cause) {
    throw toFolioError(cause);
  }
}

export async function applyPlan(
  workspaceId: WorkspaceId,
  plan: ActionPlan,
): Promise<void> {
  try {
    await invoke<void>("apply_plan", { workspaceId, planId: plan.id });
  } catch (cause) {
    throw toFolioError(cause);
  }
}
