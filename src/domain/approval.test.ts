import { describe, expect, it } from "vitest";
import {
  approvePlan,
  assertCanApply,
  replacePlan,
  restoreAfterRestart,
  restoredPlanState,
  settlePlan,
  type PlanState,
} from "./approval";
import { isFolioError } from "./errors";
import {
  editOperation,
  makePlan,
  observedPaths,
  WORKSPACE,
} from "./test-support";
import type { ActionPlan, FolioErrorCode } from "./contracts";

const BEFORE = "Deadline: October 20\n";
const AFTER = "Deadline: October 23\n";
const TARGET = "projects/project-plan.md";

function codeOf(run: () => unknown): FolioErrorCode | string {
  try {
    run();
  } catch (cause) {
    return isFolioError(cause) ? cause.code : `not-a-folio-error: ${cause}`;
  }
  return "no-error";
}

async function previewState(): Promise<{
  state: PlanState;
  plan: ActionPlan;
  current: Awaited<ReturnType<typeof observedPaths>>;
}> {
  const plan = await makePlan({
    id: "plan-1",
    operations: [await editOperation(TARGET, BEFORE, AFTER)],
  });
  return {
    plan,
    state: { status: "preview", plan },
    current: await observedPaths({ [TARGET]: BEFORE }),
  };
}

describe("approval boundaries", () => {
  it("refuses to apply a preview that was never approved", async () => {
    const { state, current } = await previewState();
    expect(codeOf(() => assertCanApply(state, current, 1_500))).toBe(
      "approvalRequired",
    );
  });

  it("refuses to approve an expired preview", async () => {
    const { state } = await previewState();
    expect(codeOf(() => approvePlan(state, 2_000))).toBe("planExpired");
  });

  it("refuses to apply an approved plan whose target changed since", async () => {
    const { state } = await previewState();
    const approved = approvePlan(state, 1_200);
    const changed = await observedPaths({ [TARGET]: "Binago ng ibang app\n" });
    expect(codeOf(() => assertCanApply(approved, changed, 1_300))).toBe(
      "targetChanged",
    );
  });

  it("re-checks expiry at application time, not only at approval", async () => {
    const { state, current } = await previewState();
    const approved = approvePlan(state, 1_200);
    expect(codeOf(() => assertCanApply(approved, current, 1_300))).toBe(
      "no-error",
    );
    expect(codeOf(() => assertCanApply(approved, current, 2_001))).toBe(
      "planExpired",
    );
  });

  it("requires fresh approval when the plan changes", async () => {
    const { state } = await previewState();
    const approved = approvePlan(state, 1_200);
    const next = await makePlan({
      id: "plan-2",
      operations: [
        await editOperation(TARGET, BEFORE, "Deadline: October 30\n"),
      ],
    });
    const replaced = replacePlan(approved, next);
    expect(replaced.status).toBe("preview");
    const current = await observedPaths({ [TARGET]: BEFORE });
    expect(codeOf(() => assertCanApply(replaced, current, 1_300))).toBe(
      "approvalRequired",
    );
  });

  it("refuses a changed plan that reuses the approved identity", async () => {
    const { state, plan } = await previewState();
    const approved = approvePlan(state, 1_200);
    const sameIdDifferentWork = await makePlan({
      id: plan.id,
      operations: [
        await editOperation(TARGET, BEFORE, "Deadline: October 30\n"),
      ],
    });
    // Reusing the identity is refused outright, and an approval carried over to
    // different operations is refused by its digest.
    expect(codeOf(() => replacePlan(approved, sameIdDifferentWork))).toBe(
      "planStateInvalid",
    );
    const forged: PlanState = {
      status: "approved",
      plan: sameIdDifferentWork,
      approval:
        approved.status === "approved"
          ? approved.approval
          : { planId: "", planDigest: "", approvedAt: 0 },
    };
    const current = await observedPaths({ [TARGET]: BEFORE });
    expect(codeOf(() => assertCanApply(forged, current, 1_300))).toBe(
      "approvalStale",
    );
  });

  it("refuses an approval token the user never gave for this plan", async () => {
    const { state, plan } = await previewState();
    const fabricated: PlanState = {
      status: "approved",
      plan,
      approval: {
        planId: plan.id,
        planDigest: "sha256:" + "0".repeat(64),
        approvedAt: 1_200,
      },
    };
    const current = await observedPaths({ [TARGET]: BEFORE });
    expect(codeOf(() => assertCanApply(fabricated, current, 1_300))).toBe(
      "approvalStale",
    );
  });

  it("records a durable outcome and refuses to apply the same plan twice", async () => {
    const { state, current } = await previewState();
    const approved = approvePlan(state, 1_200);
    const settled = settlePlan(approved, {
      attempts: [
        {
          status: "succeeded",
          historyEntryId: "history-1",
          completedAt: 1_301,
        },
      ],
      startedAt: 1_300,
      finishedAt: 1_302,
    });
    expect(settled.status).toBe("settled");
    if (settled.status === "settled") {
      expect(settled.result.stopReason).toBe("completed");
      expect(settled.result.outcomes[0].historyEntryId).toBe("history-1");
    }
    expect(codeOf(() => assertCanApply(settled, current, 1_400))).toBe(
      "approvalRequired",
    );
  });
});

describe("restart", () => {
  it("restores an unfinished preview without its approval", async () => {
    const { state } = await previewState();
    const approved = approvePlan(state, 1_200);
    const restored = restoreAfterRestart(approved.plan, true);
    expect(restored.requiresFreshApproval).toBe(true);
    const next = restoredPlanState(restored);
    expect(next.status).toBe("preview");
    const current = await observedPaths({ [TARGET]: BEFORE });
    expect(codeOf(() => assertCanApply(next, current, 1_300))).toBe(
      "approvalRequired",
    );
  });

  it("refuses to continue when the authorized folder is gone", async () => {
    const { plan } = await previewState();
    const restored = restoreAfterRestart(plan, false);
    expect(codeOf(() => restoredPlanState(restored))).toBe(
      "workspaceUnavailable",
    );
    expect(restored.plan.workspaceId).toBe(WORKSPACE);
  });
});
