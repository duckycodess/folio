import { describe, expect, it } from "vitest";
import type {
  BenchmarkResult,
  ModelDescriptor,
  ModelSetup,
  RuntimeStatus,
} from "../domain/contracts";
import {
  correctnessLabel,
  exactSize,
  installSteps,
  isGenerationReady,
  modelGroups,
  modelName,
  progressPercent,
  ramLabel,
  memorySize,
  resultsByTask,
  setupAdvice,
  totalDownloadBytes,
} from "./models";

function model(
  id: string,
  role: ModelDescriptor["role"],
  bytes: number[],
  optionalPack = false,
): ModelDescriptor {
  return {
    id,
    role,
    repo: `org/${id}-GGUF`,
    revision: "r".repeat(40),
    files: bytes.map((size, index) => ({
      path: `${id}-${index}`,
      sha256: "0".repeat(64),
      bytes: size,
    })),
    quantization: "Q4_K_M",
    license: "apache-2.0",
    runtime: role === "generation" ? "llama.cpp" : "onnxruntime",
    optionalPack,
  };
}

const E5 = model("e5", "embedding", [100, 20]);
const SMALL = model("small", "generation", [400]);
const LARGE = model("large", "generation", [1100], true);
const SETUP: ModelSetup = {
  selectedEmbedding: null,
  selectedGeneration: "small",
  hostRuntimeId: "llama-macos-arm64",
  hostRuntimeBytes: 12,
  deviceMemoryBytes: 16 * 1024 ** 3,
  availableDiskBytes: 50 * 1024 ** 3,
};
const NO_RUNTIME: RuntimeStatus = {
  id: "llama",
  version: "b1",
  installed: false,
};
const RUNTIME: RuntimeStatus = { ...NO_RUNTIME, installed: true };

describe("model setup", () => {
  it("shows a rounded size next to the exact byte count", () => {
    expect(exactSize(396_705_472)).toBe("378.3 MB (396,705,472 bytes)");
    expect(exactSize(1_107_409_472)).toBe("1.03 GB (1,107,409,472 bytes)");
  });

  it("names a model by its repository and quantization", () => {
    expect(
      modelName({ repo: "unsloth/Qwen3-0.6B-GGUF", quantization: "Q4_K_M" }),
    ).toBe("Qwen3-0.6B · Q4_K_M");
  });

  it("groups by job, recommended models first, and marks the one in use", () => {
    const groups = modelGroups([LARGE, SMALL, E5], {}, SETUP);
    expect(groups.map((group) => group.role)).toEqual([
      "embedding",
      "generation",
    ]);
    expect(
      groups[1].rows.map((row) => [row.descriptor.id, row.selected]),
    ).toEqual([
      ["small", true],
      ["large", false],
    ]);
    expect(groups[0].rows[0].downloadBytes).toBe(120);
    expect(groups[0].rows[0].state).toBeUndefined();
  });

  it("downloads the runtime first only for a writing model that lacks it", () => {
    expect(installSteps(SMALL, NO_RUNTIME)).toEqual(["runtime", "model"]);
    expect(installSteps(SMALL, RUNTIME)).toEqual(["model"]);
    expect(installSteps(E5, NO_RUNTIME)).toEqual(["model"]);
  });

  it("counts the runtime in the download, and says when its size is unknown", () => {
    expect(totalDownloadBytes(SMALL, NO_RUNTIME, SETUP)).toEqual({
      bytes: 412,
      runtimeUnknown: false,
    });
    expect(totalDownloadBytes(SMALL, RUNTIME, SETUP).bytes).toBe(400);
    expect(
      totalDownloadBytes(SMALL, NO_RUNTIME, {
        ...SETUP,
        hostRuntimeBytes: null,
      }),
    ).toEqual({ bytes: 400, runtimeUnknown: true });
  });

  it("never invents progress when the total is unknown", () => {
    expect(progressPercent(null)).toBeUndefined();
    expect(
      progressPercent({
        itemId: "x",
        file: "f",
        receivedBytes: 5,
        totalBytes: 0,
      }),
    ).toBeUndefined();
    expect(
      progressPercent({
        itemId: "x",
        file: "f",
        receivedBytes: 25,
        totalBytes: 100,
      }),
    ).toBe(25);
  });

  it("requires the selected installed writing model and its runtime", () => {
    const installed = modelGroups(
      [SMALL],
      { small: { id: "small", status: "installed" } },
      SETUP,
    );
    expect(isGenerationReady(installed, SETUP, RUNTIME)).toBe(true);
    expect(isGenerationReady(installed, SETUP, NO_RUNTIME)).toBe(false);
    expect(
      isGenerationReady(
        modelGroups(
          [SMALL],
          { small: { id: "small", status: "notInstalled" } },
          SETUP,
        ),
        SETUP,
        RUNTIME,
      ),
    ).toBe(false);
    expect(
      isGenerationReady(
        installed,
        { ...SETUP, selectedGeneration: null },
        RUNTIME,
      ),
    ).toBe(false);
  });
});

describe("onboarding's model recommendation", () => {
  const MID = model("mid", "generation", [600]);
  const GB = 1024 ** 3;
  const notInstalled = (id: string) =>
    ({ id, status: "notInstalled" }) as const;
  const states = {
    e5: notInstalled("e5"),
    small: notInstalled("small"),
    mid: notInstalled("mid"),
    large: notInstalled("large"),
  };
  const groups = modelGroups([E5, LARGE, MID, SMALL], states, SETUP);

  it("recommends the smallest non-optional model for each job, never an optional pack", () => {
    const advice = setupAdvice(groups, SETUP, NO_RUNTIME);
    expect(advice.rows.map((row) => row.descriptor.id)).toEqual([
      "e5",
      "small",
    ]);
    // 120 + 400 + the 12-byte runtime the writing model needs.
    expect(advice.downloadBytes).toBe(532);
    expect(advice.runtimeUnknown).toBe(false);
    expect(advice.withinBudget).toBe(true);
  });

  it("keeps a job's installed model in place and downloads only the rest", () => {
    const installed = modelGroups(
      [E5, LARGE, SMALL],
      { ...states, large: { id: "large", status: "installed" } },
      SETUP,
    );
    const advice = setupAdvice(installed, SETUP, RUNTIME);
    expect(advice.rows.map((row) => row.descriptor.id)).toEqual([
      "e5",
      "large",
    ]);
    expect(advice.pending.map((row) => row.descriptor.id)).toEqual(["e5"]);
    expect(advice.downloadBytes).toBe(120);
  });

  it("says when the runtime's size isn't listed instead of guessing", () => {
    const advice = setupAdvice(
      groups,
      { ...SETUP, hostRuntimeBytes: null },
      NO_RUNTIME,
    );
    expect(advice).toMatchObject({ downloadBytes: 520, runtimeUnknown: true });
  });

  it("flags low RAM and too little space only when the device reports them", () => {
    const low = {
      ...SETUP,
      deviceMemoryBytes: 4 * GB,
      availableDiskBytes: 500,
    };
    expect(setupAdvice(groups, low, RUNTIME)).toMatchObject({
      belowRamTarget: true,
      shortOfSpace: true,
    });
    const unknown = {
      ...SETUP,
      deviceMemoryBytes: null,
      availableDiskBytes: null,
    };
    expect(setupAdvice(groups, unknown, RUNTIME)).toMatchObject({
      belowRamTarget: false,
      shortOfSpace: false,
    });
    // An 8 GB computer reports a little less than 8 GB.
    const eight = { ...SETUP, deviceMemoryBytes: 7.6 * GB };
    expect(setupAdvice(groups, eight, RUNTIME).belowRamTarget).toBe(false);
    expect(memorySize(7.6 * GB)).toBe("8 GB");
  });

  it("checks the default setup against the under-1-GB target", () => {
    const big = model("big-e5", "embedding", [GB]);
    const advice = setupAdvice(
      modelGroups([big], { "big-e5": notInstalled("big-e5") }, SETUP),
      SETUP,
      RUNTIME,
    );
    expect(advice.withinBudget).toBe(false);
  });
});

describe("Model Lab results", () => {
  const run: BenchmarkResult = {
    caseId: "deadline-taglish",
    task: "interpretation",
    modelId: "small",
    revision: "r".repeat(40),
    quantization: "Q4_K_M",
    runtime: "llama.cpp b1",
    hardware: "macOS arm64, 8 GB",
    contextTokens: 2048,
    cold: true,
    taskDurationMs: 3200,
    correctness: null,
    peakProcessRamBytes: null,
    modelDiskBytes: 400,
  };

  it("keeps every task separate, including tasks with no runs", () => {
    const grouped = resultsByTask([run]);
    expect(grouped.map((group) => [group.task, group.results.length])).toEqual([
      ["retrieval", 0],
      ["interpretation", 1],
      ["summary", 0],
      ["edit", 0],
    ]);
  });

  it("labels missing measurements as such, and RAM as the process's", () => {
    expect(correctnessLabel(run)).toBe("Not graded");
    expect(ramLabel(run)).toBe("Not measured");
    expect(ramLabel({ ...run, peakProcessRamBytes: 300 * 1024 * 1024 })).toBe(
      "300.0 MB peak (Folio's model process)",
    );
    expect(ramLabel({ ...run, peakProcessRamBytes: 1 })).not.toMatch(/device/i);
  });
});
