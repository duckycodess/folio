import { describe, expect, it } from "vitest";
import {
  approvePlan,
  assertCanApply,
  markApplied,
  replacePlan,
  type PlanState,
} from "./approval";
import type { ActionPlan } from "./contracts";

const plan: ActionPlan = {
  id: "plan-1",
  workspaceId: "workspace-1",
  operations: [
    {
      kind: "edit",
      documentId: "project-plan",
      expectedContentHash: "original-hash",
      before: "October 20",
      after: "October 23",
    },
  ],
  impacts: [],
  createdAt: 100,
  expiresAt: 200,
};
const preview: PlanState = { status: "preview", plan };

describe("approval boundaries", () => {
  it("refuses writes without approval", () =>
    expect(() =>
      assertCanApply(preview, { "project-plan": "original-hash" }, 150),
    ).toThrow("APPROVAL_REQUIRED"));
  it("rejects approval after preview expires", () =>
    expect(() => approvePlan(preview, 200)).toThrow("PLAN_EXPIRED"));
  it("rejects external edits after approval", () =>
    expect(() =>
      assertCanApply(
        approvePlan(preview, 150),
        { "project-plan": "external-edit" },
        160,
      ),
    ).toThrow("TARGET_CHANGED"));
  it("checks expiry again at execution", () =>
    expect(() =>
      assertCanApply(
        approvePlan(preview, 150),
        { "project-plan": "original-hash" },
        201,
      ),
    ).toThrow("PLAN_EXPIRED"));
  it("requires fresh approval when the plan changes", () => {
    const updated = replacePlan(approvePlan(preview, 150), {
      ...plan,
      id: "plan-2",
      operations: [
        {
          ...plan.operations[0],
          kind: "edit",
          documentId: "project-plan",
          expectedContentHash: "original-hash",
          before: "October 20",
          after: "October 30",
        },
      ],
    });
    expect(updated.status).toBe("preview");
    expect(() =>
      assertCanApply(updated, { "project-plan": "original-hash" }, 160),
    ).toThrow("APPROVAL_REQUIRED");
  });
  it("records completion only with a history entry and cannot apply twice", () => {
    const approved = approvePlan(preview, 150);
    expect(() =>
      markApplied(approved, { "project-plan": "original-hash" }, 160, ""),
    ).toThrow("HISTORY_REQUIRED");
    const applied = markApplied(
      approved,
      { "project-plan": "original-hash" },
      160,
      "history-1",
    );
    expect(applied.status).toBe("applied");
    expect(() =>
      assertCanApply(applied, { "project-plan": "original-hash" }, 170),
    ).toThrow("APPROVAL_REQUIRED");
  });
});
