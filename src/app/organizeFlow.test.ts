import { describe, expect, it } from "vitest";
import type {
  ActionPlan,
  ApplyReport,
  FileOperation,
  OrganizationSuggestions,
} from "../domain/contracts";
import { folioError } from "../domain/errors";
import {
  ORGANIZE_START,
  organizeFlow,
  type OrganizeEvent,
} from "./organizeFlow";

const OPERATION: FileOperation = {
  kind: "rename",
  documentId: "w:notes/a.md",
  relativePath: "notes/a.md",
  expectedContentHash: "a".repeat(64),
  destinationRelativePath: "notes/2026-a.md",
  expectedDestination: "absent",
};
const SUGGESTIONS: OrganizationSuggestions = {
  duplicateGroups: [],
  filenames: [
    {
      documentId: OPERATION.documentId,
      relativePath: "notes/a.md",
      suggestedRelativePath: "notes/2026-a.md",
      reason: "Matches the dated names in this folder",
      operation: OPERATION,
    },
  ],
};
const PLAN = {
  id: "plan-1",
  operations: [OPERATION],
  digest: "d".repeat(64),
} as ActionPlan;
const REPORT = { batch: { outcomes: [] } } as unknown as ApplyReport;

function run(...events: OrganizeEvent[]) {
  return events.reduce(organizeFlow, ORGANIZE_START);
}

const toPreview: OrganizeEvent[] = [
  { type: "analyzeStarted", request: 1 },
  { type: "analyzed", request: 1, suggestions: SUGGESTIONS },
  { type: "toggle", documentId: OPERATION.documentId },
  { type: "prepareStarted", request: 2, operations: [OPERATION] },
  { type: "prepared", request: 2, plan: PLAN },
];

describe("the Organize flow", () => {
  it("goes analyze → suggestions → exact preview → apply → result", () => {
    expect(run(...toPreview.slice(0, 2)).stage).toBe("suggestions");
    const preview = run(...toPreview);
    expect(preview.stage).toBe("preview");
    expect(preview.plan).toBe(PLAN);
    const result = run(
      ...toPreview,
      { type: "applyStarted", request: 3 },
      { type: "applied", request: 3, report: REPORT },
    );
    expect(result.stage).toBe("result");
    expect(result.report).toBe(REPORT);
  });

  it("ignores a late reply to an earlier request", () => {
    const state = run(
      { type: "analyzeStarted", request: 1 },
      { type: "analyzeCancelled", request: 1 },
      { type: "analyzeStarted", request: 2 },
      { type: "analyzed", request: 1, suggestions: SUGGESTIONS },
    );
    expect(state.stage).toBe("analyzing");
    expect(state.suggestions).toBeNull();
  });

  it("keeps the preview when applying is refused, so it can be previewed again", () => {
    const state = run(
      ...toPreview,
      { type: "applyStarted", request: 3 },
      {
        type: "failed",
        request: 3,
        error: folioError("targetChanged", "A file changed since the preview."),
      },
    );
    expect(state.stage).toBe("preview");
    expect(state.plan).toBe(PLAN);
    expect(state.error?.code).toBe("targetChanged");
    expect(state.operations).toEqual([OPERATION]);
  });

  it("cannot apply without a plan the native core issued", () => {
    const state = run(
      { type: "analyzeStarted", request: 1 },
      { type: "analyzed", request: 1, suggestions: SUGGESTIONS },
      { type: "applyStarted", request: 2 },
    );
    expect(state.stage).toBe("suggestions");
  });

  it("returns to the suggestions when preparing is refused", () => {
    const state = run(...toPreview.slice(0, 4), {
      type: "failed",
      request: 2,
      error: folioError("destinationExists", "Taken."),
    });
    expect(state.stage).toBe("suggestions");
    expect(state.chosen).toEqual([OPERATION.documentId]);
    expect(state.error?.code).toBe("destinationExists");
  });
});

describe("stopping and starting over", () => {
  it("keeps a stopped analysis stopped even if the scan had already finished", () => {
    const state = run(
      { type: "analyzeStarted", request: 1 },
      { type: "stopAnalyze", request: 2 },
      // The scan finished just before Stop took effect.
      { type: "analyzed", request: 1, suggestions: SUGGESTIONS },
    );
    expect(state.stage).toBe("idle");
    expect(state.suggestions).toBeNull();
  });

  it("drops an apply that was still running when the flow started over", () => {
    const state = run(
      ...toPreview,
      { type: "applyStarted", request: 3 },
      // A different folder was opened, or the user pressed Done.
      { type: "reset", request: 4 },
      { type: "applied", request: 3, report: REPORT },
    );
    expect(state.stage).toBe("idle");
    expect(state.report).toBeNull();
  });
});
