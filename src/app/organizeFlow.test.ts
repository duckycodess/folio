import { describe, expect, it } from "vitest";
import type {
  ActionPlan,
  ApplyReport,
  FileChangeSuggestions,
  FileOperation,
  OrganizationSuggestions,
} from "../domain/contracts";
import { folioError } from "../domain/errors";
import {
  chosenOperations,
  ORGANIZE_START,
  organizeFlow,
  suggestionKey,
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
  { type: "toggle", key: suggestionKey("title", OPERATION.documentId) },
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
    expect(state.chosen).toEqual([
      suggestionKey("title", OPERATION.documentId),
    ]);
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

const MODEL_RENAME: FileOperation = {
  ...OPERATION,
  destinationRelativePath: "notes/trip-budget.md",
};
const MOVE: FileOperation = {
  kind: "move",
  documentId: OPERATION.documentId,
  relativePath: "notes/a.md",
  expectedContentHash: OPERATION.expectedContentHash,
  destinationRelativePath: "projects/a.md",
  expectedDestination: "absent",
};
const MODEL: FileChangeSuggestions = {
  filenames: [
    {
      documentId: OPERATION.documentId,
      relativePath: "notes/a.md",
      suggestedRelativePath: "notes/trip-budget.md",
      reason: "Named by the local AI.",
      operation: MODEL_RENAME,
      generated: { citations: [], modelId: "qwen", revision: "r" },
    },
  ],
  filenameCandidates: 1,
  naming: "named",
  destinations: [
    {
      documentId: OPERATION.documentId,
      relativePath: "notes/a.md",
      suggestedRelativePath: "projects/a.md",
      folder: "projects",
      reason: "Closer in meaning.",
      similarity: 0.9,
      currentSimilarity: 0.5,
      passage: {} as never,
      evidence: {} as never,
      provenance: "embedding",
      spaceFingerprint: "space",
      operation: MOVE,
    },
  ],
  destinationStatus: "suggested",
};

describe("the local models' renames and moves", () => {
  const assisted: OrganizeEvent[] = [
    ...toPreview.slice(0, 2),
    { type: "assistStarted", request: 7 },
    { type: "assisted", request: 7, result: MODEL },
  ];

  it("lets one change per file be chosen, and previews exactly that one", () => {
    const title = suggestionKey("title", OPERATION.documentId);
    const model = suggestionKey("model", OPERATION.documentId);
    const move = suggestionKey("move", OPERATION.documentId);
    let state = run(...assisted, { type: "toggle", key: title });
    expect(chosenOperations(state)).toEqual([OPERATION]);
    state = organizeFlow(state, { type: "toggle", key: model });
    expect(state.chosen).toEqual([model]);
    expect(chosenOperations(state)).toEqual([MODEL_RENAME]);
    state = organizeFlow(state, { type: "toggle", key: move });
    expect(chosenOperations(state)).toEqual([MOVE]);
    state = organizeFlow(state, { type: "toggle", key: move });
    expect(chosenOperations(state)).toEqual([]);
  });

  it("drops a late reply and starts over with a new analysis", () => {
    const stopped = run(
      ...toPreview.slice(0, 2),
      { type: "assistStarted", request: 7 },
      { type: "assistStopped", request: 8 },
      { type: "assisted", request: 7, result: MODEL },
    );
    expect(stopped.assist.status).toBe("idle");
    expect(stopped.assist.result).toBeNull();
    const again = run(...assisted, { type: "analyzeStarted", request: 9 });
    expect(again.assist.result).toBeNull();
    expect(again.chosen).toEqual([]);
    // The reply to the earlier models' request no longer lands.
    expect(
      organizeFlow(again, { type: "assisted", request: 7, result: MODEL })
        .assist.result,
    ).toBeNull();
  });

  it("keeps the analysis when the local models fail", () => {
    const failed = run(
      ...toPreview.slice(0, 2),
      { type: "assistStarted", request: 7 },
      {
        type: "assistFailed",
        request: 7,
        error: folioError("providerBusy", "Another request is active."),
      },
    );
    expect(failed.stage).toBe("suggestions");
    expect(failed.suggestions).toBe(SUGGESTIONS);
    expect(failed.assist.error?.message).toBe("Another request is active.");
  });
});
