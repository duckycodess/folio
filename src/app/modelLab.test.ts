import { describe, expect, it } from "vitest";
import fixture from "../../fixtures/contracts/benchmark-record.json";
import type {
  BenchmarkRecord,
  BenchmarkRunSummary,
  LabModel,
} from "../domain/contracts";
import {
  canReview,
  cpuLabel,
  evaluationCandidates,
  defaultSelection,
  durationLabel,
  isRunEnd,
  labChoices,
  nextSelection,
  memoryLabels,
  outcomeLabel,
  progressLabel,
  reconcileSelection,
  recordsByTask,
  reviewMaterial,
  runConditionRows,
  runRequest,
  sortRuns,
  toggleGeneration,
} from "./modelLab";

const record = fixture as unknown as BenchmarkRecord;

function labModel(
  id: string,
  role: LabModel["role"],
  overrides: Partial<LabModel> = {},
): LabModel {
  return {
    id,
    role,
    repo: `example/${id}-GGUF`,
    revision: "r",
    quantization: "Q4_K_M",
    modelFileBytes: 1,
    status: "installed",
    runnable: true,
    selected: false,
    catalog: "product",
    evaluationOnly: false,
    license: "example",
    licenseNote: null,
    ...overrides,
  };
}

const CANDIDATE = {
  catalog: "evaluationCandidate",
  evaluationOnly: true,
} as const;

describe("Model Lab run choices", () => {
  const models = [
    labModel("e5", "embedding", { selected: true }),
    labModel("candidate", "generation", CANDIDATE),
    labModel("qwen", "generation", { selected: true }),
    labModel("big", "generation", { runnable: false, status: "notInstalled" }),
    labModel("embed-candidate", "embedding", CANDIDATE),
  ];

  it("offers only runnable models, and never a candidate as the search model", () => {
    const choices = labChoices(models);
    expect(choices.embedding.map((model) => model.id)).toEqual(["e5"]);
    expect(choices.generation.map((model) => model.id)).toEqual([
      "qwen",
      "candidate",
    ]);
  });

  it("starts from the models in use, never an evaluation candidate", () => {
    expect(defaultSelection(labChoices(models))).toEqual({
      embeddingModelId: "e5",
      generationModelIds: ["qwen"],
    });
    const onlyCandidate = labChoices([
      labModel("e5", "embedding"),
      labModel("candidate", "generation", CANDIDATE),
    ]);
    expect(defaultSelection(onlyCandidate).generationModelIds).toEqual([]);
  });

  it("keeps the order writing models were chosen in", () => {
    let selection = defaultSelection(labChoices(models));
    selection = toggleGeneration(selection, "candidate");
    expect(selection.generationModelIds).toEqual(["qwen", "candidate"]);
    selection = toggleGeneration(selection, "qwen");
    expect(selection.generationModelIds).toEqual(["candidate"]);
  });

  it("picks the models in use on first read, then keeps the user's choice", () => {
    const choices = labChoices(models);
    const empty = { embeddingModelId: null, generationModelIds: [] };
    expect(nextSelection(empty, choices)).toEqual({
      embeddingModelId: "e5",
      generationModelIds: ["qwen"],
    });
    const chosen = { embeddingModelId: "e5", generationModelIds: [] };
    expect(nextSelection(chosen, choices)).toEqual(chosen);
  });

  it("drops a chosen model once it is no longer runnable", () => {
    const selection = {
      embeddingModelId: "e5",
      generationModelIds: ["qwen", "candidate"],
    };
    const after = labChoices([
      labModel("e5", "embedding"),
      labModel("qwen", "generation", { runnable: false }),
      labModel("candidate", "generation", CANDIDATE),
    ]);
    expect(reconcileSelection(selection, after).generationModelIds).toEqual([
      "candidate",
    ]);
  });

  it("says what is missing instead of starting a run that would be refused", () => {
    expect(
      runRequest(defaultSelection(labChoices([])), labChoices([])),
    ).toEqual({ blocked: "Install a search model to compare models." });
    const noWriter = labChoices([labModel("e5", "embedding")]);
    expect(runRequest(defaultSelection(noWriter), noWriter)).toEqual({
      blocked: "Install a writing model to compare models.",
    });
    const choices = labChoices(models);
    expect(
      runRequest({ embeddingModelId: "e5", generationModelIds: [] }, choices),
    ).toEqual({ blocked: "Choose at least one writing model." });
    expect(runRequest(defaultSelection(choices), choices)).toEqual({
      request: { embeddingModelId: "e5", generationModelIds: ["qwen"] },
    });
  });
});

describe("Model Lab progress", () => {
  const models = [labModel("qwen", "generation")];
  const at = (step: string, extra = {}) => ({
    runId: "lab-1",
    step,
    caseId: null,
    modelId: null,
    ...extra,
  });

  it("names the model and case being measured", () => {
    expect(progressLabel(at("starting", { modelId: "qwen" }), models)).toBe(
      "Starting qwen · Q4_K_M",
    );
    expect(
      progressLabel(
        at("case", { modelId: "qwen", caseId: "deadline" }),
        models,
      ),
    ).toBe("qwen · Q4_K_M: case deadline");
    expect(progressLabel(null, models)).toBe("Starting the comparison");
  });

  it("ends only on finished, cancelled or failed", () => {
    expect(
      ["finished", "cancelled", "failed"].map((s) => isRunEnd(at(s))),
    ).toEqual([true, true, true]);
    expect(isRunEnd(at("case"))).toBe(false);
  });
});

describe("Model Lab records", () => {
  it("never reads a failure to answer as a wrong answer or as ungraded", () => {
    expect(outcomeLabel({ ...record, outcomeKind: "timedOut" })).toBe(
      "Timed out",
    );
    expect(
      outcomeLabel({
        ...record,
        outcomeKind: "runtimeError",
        correctness: false,
      }),
    ).toBe("Model error");
    expect(
      outcomeLabel({ ...record, task: "retrieval", correctness: false }),
    ).toBe("Incorrect");
  });

  it("shows a summary as not reviewed until a person reviews this exact output", () => {
    expect(outcomeLabel(record)).toBe("Not reviewed");
    const review = {
      status: "partiallyCorrect" as const,
      reviewer: "TJ",
      reviewedAt: 2,
      outputSha256: "other output",
    };
    expect(outcomeLabel({ ...record, reviews: [review] })).toBe("Not reviewed");
    expect(
      outcomeLabel({
        ...record,
        reviews: [{ ...review, outputSha256: record.outputSha256 }],
      }),
    ).toBe("Reviewed: partly correct");
  });

  it("labels memory per process, with the reason when it wasn't read", () => {
    expect(memoryLabels(record)).toEqual([
      "model server: not measured (contract example: no process was measured)",
    ]);
    const measured = {
      ...record,
      memory: [{ ...record.memory[0], peakBytes: 300 * 1024 * 1024 }],
    };
    expect(memoryLabels(measured)).toEqual(["model server: 300.0 MB peak"]);
    expect(memoryLabels({ ...record, memory: [] })).toEqual(["Not measured"]);
    expect(memoryLabels(measured).join(" ")).not.toMatch(/device/i);
  });

  it("only calls a run CPU-only when the server's output confirmed it", () => {
    const backend = record.runtimeDetail.backend!;
    const withBackend = (next: Partial<typeof backend> | undefined) => ({
      ...record,
      runtimeDetail: {
        ...record.runtimeDetail,
        backend: next && { ...backend, ...next },
      },
    });
    expect(cpuLabel(record)).toBe("CPU only (confirmed)");
    expect(cpuLabel(withBackend({ cpuOnlyVerified: null }))).toBe(
      "CPU only requested; not confirmed",
    );
    expect(
      cpuLabel(withBackend({ cpuOnlyVerified: false, gpuLayersOffloaded: 12 })),
    ).toBe("GPU used (12 layers)");
    expect(cpuLabel(withBackend({ gpuOffload: "runtimeDefault" }))).toBe(
      "Runtime's default",
    );
    expect(cpuLabel(withBackend(undefined))).toBe(
      "In-process (no model server)",
    );
  });

  it("keeps startup time apart from the first request's time", () => {
    expect(durationLabel(record)).toBe("1.2 s (+4.3 s startup)");
    expect(durationLabel({ ...record, cold: false })).toBe("1.2 s");
  });

  it("keeps every task separate, including tasks with no records", () => {
    expect(
      recordsByTask([record]).map((group) => [
        group.task,
        group.records.length,
      ]),
    ).toEqual([
      ["retrieval", 0],
      ["interpretation", 0],
      ["summary", 1],
      ["edit", 0],
    ]);
  });
});

describe("Model Lab runs", () => {
  const run: BenchmarkRunSummary = {
    runId: "lab-1",
    status: "completed",
    requestedModelIds: ["e5", "qwen"],
    suite: record.suite,
    corpusSha256: "c",
    host: record.host,
    serverSettings: { startupWarmup: "disabled", cachePrompt: false },
    startedAt: 1,
    endedAt: 2,
    indexBuildMs: null,
    schemaVersion: 1,
  };

  it("lists the newest run first", () => {
    const newer = { ...run, runId: "lab-2", startedAt: 5 };
    expect(sortRuns([run, newer]).map((r) => r.runId)).toEqual([
      "lab-2",
      "lab-1",
    ]);
  });

  it("describes installed RAM as capacity and an unfrozen suite as such", () => {
    const rows = Object.fromEntries(
      runConditionRows(run).map((row) => [row.label, row.value]),
    );
    expect(rows.Computer).toContain("8.0 GB RAM installed");
    expect(rows["Test set"]).toContain("not frozen");
    expect(rows["Building test passages"]).toBeUndefined();
    expect(runConditionRows({ ...run, indexBuildMs: 2500 }).at(-1)?.value).toBe(
      "2.5 s",
    );
  });
});

describe("Model Lab candidates and reviews", () => {
  it("lists only evaluation candidates, installed or not", () => {
    const models = [
      labModel("qwen", "generation"),
      labModel("sea", "generation", { ...CANDIDATE, status: "notInstalled" }),
    ];
    expect(evaluationCandidates(models).map((model) => model.id)).toEqual([
      "sea",
    ]);
  });

  it("offers a review only for a summary that was produced", () => {
    expect(canReview(record)).toBe(true);
    expect(canReview({ ...record, outcomeKind: "timedOut" })).toBe(false);
    expect(canReview({ ...record, task: "interpretation" })).toBe(false);
  });

  it("reads what the reviewer needs from the recorded output", () => {
    const output = {
      request: "Ibuod mo ito",
      document: "notes/plano.md",
      suppliedPassages: [{ text: "Unang talata." }, { start: 1 }, null],
      result: { text: "Buod ng plano." },
    };
    expect(reviewMaterial({ ...record, output })).toEqual({
      request: "Ibuod mo ito",
      document: "notes/plano.md",
      summary: "Buod ng plano.",
      passages: ["Unang talata."],
    });
  });

  it("never fails on an output shaped differently than expected", () => {
    for (const output of [
      null,
      "text",
      3,
      { result: "x", suppliedPassages: {} },
    ]) {
      expect(reviewMaterial({ ...record, output })).toEqual({
        request: null,
        document: null,
        summary: null,
        passages: [],
      });
    }
  });
});
