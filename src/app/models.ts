import type {
  BenchmarkResult,
  DownloadProgress,
  ModelDescriptor,
  ModelInstallState,
  ModelRole,
  ModelSetup,
  RuntimeStatus,
} from "../domain/contracts";

/** What each kind of model does, in the user's words. */
export const ROLE_LABELS: Record<
  ModelRole,
  { title: string; purpose: string }
> = {
  embedding: {
    title: "Search model",
    purpose: "Finds passages by meaning, in English, Filipino and Taglish.",
  },
  generation: {
    title: "Writing model",
    purpose: "Writes summaries and reads requests in Ask & Act.",
  },
};

/** "Qwen3-0.6B · Q4_K_M": the repository's model name and its quantization. */
export function modelName(
  descriptor: Pick<ModelDescriptor, "repo" | "quantization">,
): string {
  const base = (descriptor.repo.split("/").at(-1) ?? descriptor.repo).replace(
    /-GGUF$/i,
    "",
  );
  return `${base} · ${descriptor.quantization}`;
}

/** The exact bytes a model downloads: every pinned file, nothing estimated. */
export function downloadBytes(descriptor: { files: { bytes: number }[] }) {
  return descriptor.files.reduce((total, file) => total + file.bytes, 0);
}

/** "396.7 MB (396,705,472 bytes)": rounded for reading, exact for checking. */
export function exactSize(bytes: number): string {
  const megabytes = bytes / (1024 * 1024);
  const rounded =
    megabytes >= 1024
      ? `${(megabytes / 1024).toFixed(2)} GB`
      : `${megabytes.toFixed(1)} MB`;
  return `${rounded} (${bytes.toLocaleString("en-US")} bytes)`;
}

export interface ModelRow {
  descriptor: ModelDescriptor;
  /** Unknown until `verify_model` answers. */
  state: ModelInstallState | undefined;
  selected: boolean;
  downloadBytes: number;
}

export interface ModelGroup {
  role: ModelRole;
  rows: ModelRow[];
}

/** Models grouped by what they do; the recommended (non-optional) ones first. */
export function modelGroups(
  descriptors: ModelDescriptor[],
  states: Record<string, ModelInstallState>,
  setup: ModelSetup | null,
): ModelGroup[] {
  const selected = new Set(
    [setup?.selectedEmbedding, setup?.selectedGeneration].filter(Boolean),
  );
  return (["embedding", "generation"] as const)
    .map((role) => ({
      role,
      rows: descriptors
        .filter((descriptor) => descriptor.role === role)
        .sort((a, b) => Number(a.optionalPack) - Number(b.optionalPack))
        .map((descriptor) => ({
          descriptor,
          state: states[descriptor.id],
          selected: selected.has(descriptor.id),
          downloadBytes: downloadBytes(descriptor),
        })),
    }))
    .filter((group) => group.rows.length > 0);
}

export type InstallStep = "runtime" | "model";

/**
 * A writing model also needs the llama.cpp runtime for this computer; it is
 * downloaded first, and only when it isn't installed yet.
 */
export function installSteps(
  descriptor: Pick<ModelDescriptor, "role">,
  runtime: RuntimeStatus | null,
): InstallStep[] {
  return descriptor.role === "generation" && !runtime?.installed
    ? ["runtime", "model"]
    : ["model"];
}

/** The first download's size, plus the runtime's when it comes too. */
export function totalDownloadBytes(
  descriptor: ModelDescriptor,
  runtime: RuntimeStatus | null,
  setup: ModelSetup | null,
): { bytes: number; runtimeUnknown: boolean } {
  const steps = installSteps(descriptor, runtime);
  const runtimeBytes = setup?.hostRuntimeBytes ?? null;
  const needsRuntime = steps.includes("runtime");
  return {
    bytes: downloadBytes(descriptor) + (needsRuntime ? (runtimeBytes ?? 0) : 0),
    runtimeUnknown: needsRuntime && runtimeBytes === null,
  };
}

/** Whole-item progress, or `undefined` when the total isn't known. */
export function progressPercent(
  progress: DownloadProgress | null,
): number | undefined {
  if (!progress || progress.totalBytes <= 0) return undefined;
  return Math.min(100, (progress.receivedBytes / progress.totalBytes) * 100);
}

/* -------------------------------------------------------------- Model Lab */

export const BENCHMARK_TASKS: BenchmarkResult["task"][] = [
  "retrieval",
  "interpretation",
  "summary",
  "edit",
];

export const TASK_LABELS: Record<BenchmarkResult["task"], string> = {
  retrieval: "Finding files",
  interpretation: "Reading requests",
  summary: "Summaries",
  edit: "Edits",
};

/**
 * Results kept per task, never combined into one score. Every task appears,
 * so a task with no recorded runs shows as such instead of disappearing.
 */
export function resultsByTask(results: BenchmarkResult[]) {
  return BENCHMARK_TASKS.map((task) => ({
    task,
    results: results.filter((result) => result.task === task),
  }));
}

export function correctnessLabel(result: BenchmarkResult): string {
  return result.correctness === null
    ? "Not graded"
    : result.correctness
      ? "Correct"
      : "Incorrect";
}

/** Peak RAM of the measured process. It is never the whole device's RAM. */
export function ramLabel(result: BenchmarkResult): string {
  return result.peakProcessRamBytes === null
    ? "Not measured"
    : `${exactSize(result.peakProcessRamBytes).split(" (")[0]} peak (Folio's model process)`;
}
