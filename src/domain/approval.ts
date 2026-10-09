import type {
  ActionPlan,
  Approval,
  BatchResult,
  RestoredPreview,
} from "./contracts";
import { folioError } from "./errors";
import {
  assertApprovalMatches,
  preflightPlan,
  settleBatch,
  type ObservedPaths,
  type SettleBatchInput,
} from "./plan";

/**
 * Deterministic approval state. The native core holds the authoritative copy:
 * it issues plan identities and digests, and it refuses an approval it did not
 * issue. This module exists so both sides agree on the transitions, and so the
 * UI can show the same state without inventing authority it does not have.
 */
export type PlanState =
  | { status: "preview"; plan: ActionPlan }
  | { status: "approved"; plan: ActionPlan; approval: Approval }
  | {
      status: "settled";
      plan: ActionPlan;
      approval: Approval;
      result: BatchResult;
    };

function assertCurrent(plan: ActionPlan, now: number): void {
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
}

/** Approve exactly this plan, binding the approval to its digest. */
export function approvePlan(state: PlanState, now: number): PlanState {
  if (state.status !== "preview") {
    throw folioError(
      "planStateInvalid",
      "Only a current preview can be approved.",
      { status: state.status },
    );
  }
  assertCurrent(state.plan, now);
  return {
    status: "approved",
    plan: structuredClone(state.plan),
    approval: {
      planId: state.plan.id,
      planDigest: state.plan.digest,
      approvedAt: now,
    },
  };
}

/**
 * The last gate before any file changes: an approval for this exact plan, a
 * plan that has not expired, and a clean preflight over every target.
 */
export function assertCanApply(
  state: PlanState,
  observed: ObservedPaths,
  now: number,
): void {
  if (state.status !== "approved") {
    throw folioError(
      "approvalRequired",
      "Approve this exact plan before any file changes.",
      { status: state.status },
    );
  }
  assertApprovalMatches(state.plan, state.approval);
  preflightPlan(state.plan, observed, now);
}

/** Record what actually happened, one durable outcome per operation. */
export function settlePlan(
  state: PlanState,
  attempts: Omit<SettleBatchInput, "plan" | "approval">,
): PlanState {
  if (state.status !== "approved") {
    throw folioError(
      "approvalRequired",
      "A batch can only be recorded against an approved plan.",
      { status: state.status },
    );
  }
  const result = settleBatch({
    ...attempts,
    plan: state.plan,
    approval: state.approval,
  });
  return {
    status: "settled",
    plan: state.plan,
    approval: state.approval,
    result,
  };
}

/** A changed plan is a new preview; the previous approval does not carry over. */
export function replacePlan(state: PlanState, next: ActionPlan): PlanState {
  if (state.status === "settled") {
    throw folioError(
      "planStateInvalid",
      "This plan was already applied. Prepare a new one.",
      { planId: state.plan.id },
    );
  }
  if (next.id === state.plan.id) {
    throw folioError(
      "planStateInvalid",
      "A changed plan needs its own identity.",
      { planId: state.plan.id },
    );
  }
  return { status: "preview", plan: structuredClone(next) };
}

/**
 * Restart policy: an unfinished preview comes back as a preview, its approval
 * is dropped, folder access is revalidated, and nothing is applied
 * automatically. A settled batch is already durable and is not restored as
 * pending work.
 */
export function restoreAfterRestart(
  plan: ActionPlan,
  workspaceAvailable: boolean,
): RestoredPreview {
  return {
    plan: structuredClone(plan),
    workspaceAvailable,
    requiresFreshApproval: true,
  };
}

/** The restored preview state a restart hands back to the user. */
export function restoredPlanState(restored: RestoredPreview): PlanState {
  if (!restored.workspaceAvailable) {
    throw folioError(
      "workspaceUnavailable",
      "That folder is no longer available. Choose it again to continue.",
      { workspaceId: restored.plan.workspaceId },
    );
  }
  return { status: "preview", plan: structuredClone(restored.plan) };
}
