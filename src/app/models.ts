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

/** Whether the selected writing model and its host runtime can serve a request. */
export function isGenerationReady(
  groups: ModelGroup[],
  setup: ModelSetup | null,
  runtime: RuntimeStatus | null,
): boolean {
  const selected = setup?.selectedGeneration;
  if (!selected || runtime?.installed !== true) return false;
  return groups.some(
    (group) =>
      group.role === "generation" &&
      group.rows.some(
        (row) =>
          row.descriptor.id === selected &&
          row.selected &&
          row.state?.status === "installed",
      ),
  );
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

/**
 * Folio targets computers with 8 GB of RAM (an unmeasured target). Operating
 * systems report a little less than the installed amount, so an 8 GB computer
 * that reports 7.6 GB still counts.
 */
const RAM_TARGET_BYTES = 7.5 * 1024 ** 3;

/** The default setup stays within Folio's under-1-GB install target. */
const DEFAULT_BUDGET_BYTES = 1024 ** 3;

export interface SetupAdvice {
  /**
   * One model per job: the installed one (the one in use first), or else the
   * smallest recommended (non-optional) one.
   */
  rows: ModelRow[];
  /** Those rows still to download. */
  pending: ModelRow[];
  /** Exact bytes the pending downloads take, the runtime's when needed. */
  downloadBytes: number;
  /** A runtime download is needed but the manifest doesn't list its size. */
  runtimeUnknown: boolean;
  withinBudget: boolean;
  /** Known, and below what Folio targets. Unknown RAM is never "below". */
  belowRamTarget: boolean;
  /** Known, and smaller than the downloads. Unknown space is never "short". */
  shortOfSpace: boolean;
}

/**
 * What onboarding recommends. A job that has a model installed keeps it, so a
 * model just set up stays in place; otherwise its smallest recommended model.
 * Larger optional packs are never recommended; they stay a choice with their
 * sizes shown.
 */
export function setupAdvice(
  groups: ModelGroup[],
  setup: ModelSetup | null,
  runtime: RuntimeStatus | null,
): SetupAdvice {
  const rows = groups.flatMap((group) => {
    const installed = group.rows
      .filter((row) => row.state?.status === "installed")
      .sort((a, b) => Number(b.selected) - Number(a.selected))[0];
    const smallest = group.rows
      .filter((row) => !row.descriptor.optionalPack)
      .sort((a, b) => a.downloadBytes - b.downloadBytes)[0];
    const row = installed ?? smallest;
    return row ? [row] : [];
  });
  const pending = rows.filter((row) => row.state?.status !== "installed");
  const needsRuntime = pending.some((row) =>
    installSteps(row.descriptor, runtime).includes("runtime"),
  );
  const runtimeBytes = needsRuntime ? setup?.hostRuntimeBytes : 0;
  const bytes =
    pending.reduce((total, row) => total + row.downloadBytes, 0) +
    (runtimeBytes ?? 0);
  const memory = setup?.deviceMemoryBytes ?? null;
  const free = setup?.availableDiskBytes ?? null;
  return {
    rows,
    pending,
    downloadBytes: bytes,
    runtimeUnknown: needsRuntime && runtimeBytes == null,
    withinBudget: bytes < DEFAULT_BUDGET_BYTES,
    belowRamTarget: memory !== null && memory < RAM_TARGET_BYTES,
    shortOfSpace: free !== null && free < bytes,
  };
}

/** "16 GB": whole gigabytes for RAM, as computers are sold. */
export function memorySize(bytes: number): string {
  return `${Math.round(bytes / 1024 ** 3)} GB`;
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
