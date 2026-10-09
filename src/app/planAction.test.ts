import { describe, expect, it } from "vitest";
import type {
  ActionPlan,
  ApplyReport,
  FileOperation,
  UndoPreflight,
  UndoReport,
} from "../domain/contracts";
import { folioError } from "../domain/errors";
import {
  PLAN_ACTION_START,
  planAction,
  type PlanActionEvent,
} from "./planAction";

const EDIT: FileOperation = {
  kind: "edit",
  documentId: "w:projects/plan.md",
  relativePath: "projects/plan.md",
  expectedContentHash: `sha256:${"a".repeat(64)}`,
  after: "Deadline: October 23\n",
};
const PLAN = {
  id: "plan-1",
  operations: [EDIT],
  impacts: [],
  digest: `sha256:${"d".repeat(64)}`,
} as unknown as ActionPlan;
const REPORT: ApplyReport = {
  batch: {
    planId: PLAN.id,
    planDigest: PLAN.digest,
    startedAt: 1,
    finishedAt: 2,
    stopReason: "completed",
    outcomes: [
      { operationIndex: 0, status: "succeeded", historyEntryId: "h-1" },
    ],
  },
  historySettled: true,
  indexRefreshed: true,
};
const UNDOABLE: UndoPreflight = {
  planId: PLAN.id,
  entryIds: ["h-1"],
  conflicts: [],
  undoable: true,
};
const CONFLICT: UndoPreflight = {
  ...UNDOABLE,
  undoable: false,
  conflicts: [
    {
      historyEntryId: "h-1",
      relativePath: "projects/plan.md",
      observedContentHash: `sha256:${"e".repeat(64)}`,
      reason: "externallyModified",
    },
  ],
};
const UNDONE: UndoReport = {
  planId: PLAN.id,
  undoneEntryIds: ["h-1"],
  remainingEntryIds: [],
  indexRefreshed: true,
};

function run(...events: PlanActionEvent[]) {
  return events.reduce(planAction, PLAN_ACTION_START);
}

const toPreview: PlanActionEvent[] = [
  { type: "prepareStarted", request: 1 },
  { type: "prepared", request: 1, plan: PLAN },
];
const toResult: PlanActionEvent[] = [
  ...toPreview,
  { type: "applyStarted", request: 2 },
  { type: "applied", request: 2, report: REPORT },
];

describe("a single-plan action", () => {
  it("goes preview → apply → result → Undo preview → undone", () => {
    expect(run(...toPreview)).toMatchObject({ stage: "preview", plan: PLAN });
    expect(run(...toResult)).toMatchObject({ stage: "result", report: REPORT });
    const undone = run(
      ...toResult,
      { type: "undoPreviewStarted", request: 3 },
      { type: "undoPreviewed", request: 3, preflight: UNDOABLE },
      { type: "undoStarted", request: 4 },
      { type: "undone", request: 4, report: UNDONE },
    );
    expect(undone).toMatchObject({ stage: "undone", undoReport: UNDONE });
    expect(undone.report).toBe(REPORT);
  });

  it("never applies without a native plan on screen", () => {
    expect(run({ type: "applyStarted", request: 1 })).toEqual(
      PLAN_ACTION_START,
    );
    // Still preparing: there's no plan to approve yet.
    const preparing = run(toPreview[0], { type: "applyStarted", request: 2 });
    expect(preparing.stage).toBe("preparing");
    // A preview that failed leaves nothing to approve.
    const refused = run(
      toPreview[0],
      { type: "failed", request: 1, error: folioError("targetChanged", "x") },
      { type: "applyStarted", request: 2 },
    );
    expect(refused.stage).toBe("idle");
    expect(refused.plan).toBeNull();
  });

  it("reaches a result only from the native report for the apply in flight", () => {
    // A report with no apply started (or for another request) is ignored.
    expect(
      run(...toPreview, { type: "applied", request: 1, report: REPORT }).stage,
    ).toBe("preview");
    const stale = run(
      ...toPreview,
      { type: "applyStarted", request: 2 },
      { type: "applied", request: 1, report: REPORT },
    );
    expect(stale.stage).toBe("applying");
    expect(stale.report).toBeNull();
  });

  it("ignores a late plan once the user previewed again or started over", () => {
    const again = run(
      toPreview[0],
      { type: "prepareStarted", request: 2 },
      { type: "prepared", request: 1, plan: PLAN },
    );
    expect(again.stage).toBe("preparing");
    expect(again.plan).toBeNull();

    const reset = run(
      ...toPreview,
      { type: "applyStarted", request: 2 },
      { type: "reset", request: 3 },
      { type: "applied", request: 2, report: REPORT },
    );
    expect(reset).toEqual({ ...PLAN_ACTION_START, request: 3 });
  });

  it("keeps the preview when applying is refused, and needs a fresh one to approve", () => {
    const state = run(
      ...toPreview,
      { type: "applyStarted", request: 2 },
      {
        type: "failed",
        request: 2,
        error: folioError("approvalStale", "Out of date."),
      },
    );
    expect(state.stage).toBe("preview");
    expect(state.plan).toBe(PLAN);
    expect(state.report).toBeNull();
    expect(state.error?.code).toBe("approvalStale");
    // The refused plan can't be approved again as it stands.
    expect(planAction(state, { type: "applyStarted", request: 3 })).toBe(state);
  });

  it("doesn't leave a running apply for a new preview", () => {
    const applying = run(...toPreview, { type: "applyStarted", request: 2 });
    expect(planAction(applying, { type: "prepareStarted", request: 3 })).toBe(
      applying,
    );
  });

  it("keeps everything as it was when Undo finds a conflict, claiming nothing undone", () => {
    const blocked = run(
      ...toResult,
      { type: "undoPreviewStarted", request: 3 },
      { type: "undoPreviewed", request: 3, preflight: CONFLICT },
      { type: "undoStarted", request: 4 },
    );
    expect(blocked.stage).toBe("undoPreview");
    expect(blocked.undoReport).toBeNull();

    // A conflict the native core finds at confirm time changes nothing either.
    const refused = run(
      ...toResult,
      { type: "undoPreviewStarted", request: 3 },
      { type: "undoPreviewed", request: 3, preflight: UNDOABLE },
      { type: "undoStarted", request: 4 },
      {
        type: "failed",
        request: 4,
        error: folioError("undoConflict", "Changed."),
      },
    );
    expect(refused.stage).toBe("undoPreview");
    expect(refused.undoReport).toBeNull();
    expect(refused.report).toBe(REPORT);
    expect(refused.error?.code).toBe("undoConflict");
    // It can't be confirmed again until Undo is previewed again.
    expect(planAction(refused, { type: "undoStarted", request: 5 })).toBe(
      refused,
    );
  });

  it("offers Undo only when something was saved with history", () => {
    const nothingSaved: ApplyReport = {
      ...REPORT,
      batch: {
        ...REPORT.batch,
        stopReason: "failed",
        outcomes: [
          {
            operationIndex: 0,
            status: "failed",
            error: { code: "targetChanged", message: "Changed." },
          },
        ],
      },
    };
    const state = run(
      ...toPreview,
      { type: "applyStarted", request: 2 },
      { type: "applied", request: 2, report: nothingSaved },
      { type: "undoPreviewStarted", request: 3 },
    );
    expect(state.stage).toBe("result");
  });

  it("drops an Undo preflight that arrives after the user closed it", () => {
    const state = run(
      ...toResult,
      { type: "undoPreviewStarted", request: 3 },
      { type: "closeUndo", request: 4 },
      { type: "undoPreviewed", request: 3, preflight: UNDOABLE },
    );
    expect(state.stage).toBe("result");
    expect(state.undoPreflight).toBeNull();
  });
});
