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
  modelGroups,
  modelName,
  progressPercent,
  ramLabel,
  resultsByTask,
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
