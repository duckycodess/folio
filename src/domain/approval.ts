import type { ActionPlan } from "./contracts";

/** State logic only. The native core must store authoritative plans/approval. */
export type PlanState =
  | { status: "preview"; plan: ActionPlan }
  | { status: "approved"; plan: ActionPlan; approvedAt: number }
  | {
      status: "applied";
      plan: ActionPlan;
      approvedAt: number;
      appliedAt: number;
      historyEntryId: string;
    };

function assertCurrent(plan: ActionPlan, now: number): void {
  if (now >= plan.expiresAt || now < plan.createdAt)
    throw new Error("PLAN_EXPIRED");
  if (plan.operations.length === 0) throw new Error("EMPTY_PLAN");
}

export function approvePlan(state: PlanState, now: number): PlanState {
  if (state.status !== "preview") throw new Error("INVALID_PLAN_STATE");
  assertCurrent(state.plan, now);
  return {
    status: "approved",
    plan: structuredClone(state.plan),
    approvedAt: now,
  };
}

export function assertCanApply(
  state: PlanState,
  observedHashes: Record<string, string>,
  now: number,
): void {
  if (state.status !== "approved") throw new Error("APPROVAL_REQUIRED");
  assertCurrent(state.plan, now);
  for (const operation of state.plan.operations) {
    if (
      operation.kind !== "create" &&
      observedHashes[operation.documentId] !== operation.expectedContentHash
    ) {
      throw new Error("TARGET_CHANGED");
    }
  }
}

export function markApplied(
  state: PlanState,
  observedHashes: Record<string, string>,
  now: number,
  historyEntryId: string,
): PlanState {
  assertCanApply(state, observedHashes, now);
  if (state.status !== "approved") throw new Error("APPROVAL_REQUIRED");
  if (!historyEntryId.trim()) throw new Error("HISTORY_REQUIRED");
  return { ...state, status: "applied", appliedAt: now, historyEntryId };
}

export function replacePlan(state: PlanState, next: ActionPlan): PlanState {
  if (state.status === "applied") throw new Error("ALREADY_APPLIED");
  if (next.id === state.plan.id) throw new Error("NEW_PLAN_ID_REQUIRED");
  return { status: "preview", plan: structuredClone(next) };
}
