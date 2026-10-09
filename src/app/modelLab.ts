import type {
  BenchmarkMemory,
  BenchmarkRecord,
  BenchmarkRunSummary,
  LabModel,
  LabProgress,
  LabRunRequest,
} from "../domain/contracts";
import { exactSize, modelName } from "./models";

/* ------------------------------------------------------------ run choices */

export interface LabChoices {
  /** Installed, verified product search models; a run measures one. */
  embedding: LabModel[];
  /** Installed, verified writing models, product ones first. */
  generation: LabModel[];
}

/**
 * What a run can measure: only installed, hash-verified models. The native
 * core refuses an evaluation candidate as the search model, so it is never
 * offered there.
 */
export function labChoices(models: LabModel[]): LabChoices {
  const runnable = models.filter((model) => model.runnable);
  return {
    embedding: runnable.filter(
      (model) => model.role === "embedding" && model.catalog === "product",
    ),
    generation: runnable
      .filter((model) => model.role === "generation")
      .sort((a, b) => Number(a.evaluationOnly) - Number(b.evaluationOnly)),
  };
}

export interface LabSelection {
  embeddingModelId: string | null;
  /** In the order they will run. */
  generationModelIds: string[];
}

/**
 * The starting choice: the search model in use (or the only one), and the
 * writing model in use. Nothing is picked that isn't runnable.
 */
export function defaultSelection(choices: LabChoices): LabSelection {
  const embedding =
    choices.embedding.find((model) => model.selected) ?? choices.embedding[0];
  const generation =
    choices.generation.find((model) => model.selected) ??
    choices.generation.find((model) => !model.evaluationOnly);
  return {
    embeddingModelId: embedding?.id ?? null,
    generationModelIds: generation ? [generation.id] : [],
  };
}

/** Drops chosen models that are no longer runnable, e.g. after a removal. */
export function reconcileSelection(
  selection: LabSelection,
  choices: LabChoices,
): LabSelection {
  const embedding = choices.embedding.some(
    (model) => model.id === selection.embeddingModelId,
  )
    ? selection.embeddingModelId
    : (defaultSelection(choices).embeddingModelId ?? null);
  const generation = selection.generationModelIds.filter((id) =>
    choices.generation.some((model) => model.id === id),
  );
  return { embeddingModelId: embedding, generationModelIds: generation };
}

/**
 * The choice after the models are read: the defaults the first time, then
 * what the user chose, minus anything no longer runnable.
 */
export function nextSelection(
  current: LabSelection,
  choices: LabChoices,
): LabSelection {
  const untouched =
    current.embeddingModelId === null && !current.generationModelIds.length;
  return untouched
    ? defaultSelection(choices)
    : reconcileSelection(current, choices);
}

/** Adds or removes one writing model, keeping the order they were chosen in. */
export function toggleGeneration(
  selection: LabSelection,
  modelId: string,
): LabSelection {
  const chosen = selection.generationModelIds.includes(modelId);
  return {
    ...selection,
    generationModelIds: chosen
      ? selection.generationModelIds.filter((id) => id !== modelId)
      : [...selection.generationModelIds, modelId],
  };
}

/**
 * The request to send, or why the run can't start yet. The native core
 * checks the same rules; this only keeps Start from being a dead end.
 */
export function runRequest(
  selection: LabSelection,
  choices: LabChoices,
): { request: LabRunRequest } | { blocked: string } {
  if (!choices.embedding.length) {
    return { blocked: "Install a search model to compare models." };
  }
  if (!choices.generation.length) {
    return { blocked: "Install a writing model to compare models." };
  }
  if (!selection.embeddingModelId) {
    return { blocked: "Choose a search model." };
  }
  if (!selection.generationModelIds.length) {
    return { blocked: "Choose at least one writing model." };
  }
  return {
    request: {
      embeddingModelId: selection.embeddingModelId,
      generationModelIds: selection.generationModelIds,
    },
  };
}

/* --------------------------------------------------------------- progress */

/** `finished`, `cancelled` and `failed` end a run; nothing follows them. */
export function isRunEnd(progress: LabProgress): boolean {
  return ["finished", "cancelled", "failed"].includes(progress.step);
}

/** What the running step is doing, in words. Unknown steps are shown as is. */
export function progressLabel(
  progress: LabProgress | null,
  models: LabModel[],
): string {
  if (!progress) return "Starting the comparison";
  const model = models.find((candidate) => candidate.id === progress.modelId);
  const name = model ? modelName(model) : progress.modelId;
  switch (progress.step) {
    case "indexing":
      return `Building the test passages with ${name}`;
    case "retrieval":
      return `Finding files: case ${progress.caseId}`;
    case "starting":
      return `Starting ${name}`;
    case "case":
      return `${name}: case ${progress.caseId}`;
    case "finished":
      return "Comparison finished";
    case "cancelled":
      return "Comparison stopped";
    case "failed":
      return "Comparison stopped by an error";
    default:
      return progress.step;
  }
}

/* ------------------------------------------------------------------- runs */

export const RUN_STATUS_LABELS: Record<BenchmarkRunSummary["status"], string> =
  {
    running: "Running",
    completed: "Finished",
    cancelled: "Stopped",
    failed: "Failed",
  };

/** Newest first. */
export function sortRuns(runs: BenchmarkRunSummary[]): BenchmarkRunSummary[] {
  return [...runs].sort((a, b) => b.startedAt - a.startedAt);
}

export function runLabel(run: BenchmarkRunSummary): string {
  const started = new Date(run.startedAt).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  return `${started} · ${RUN_STATUS_LABELS[run.status]}`;
}

/* ---------------------------------------------------------------- records */

/**
 * The case's result. A failure to produce an answer says why, so it isn't
 * read as a wrong answer or as "not graded yet". Summaries are never graded
 * automatically; a person's latest review is shown instead.
 */
export function outcomeLabel(record: BenchmarkRecord): string {
  switch (record.outcomeKind) {
    case "invalidModelOutput":
      return "Unusable answer";
    case "timedOut":
      return "Timed out";
    case "runtimeError":
      return "Model error";
    case "cancelled":
      return "Stopped";
    case "valid":
      break;
  }
  if (record.correctness !== null) {
    return record.correctness ? "Correct" : "Incorrect";
  }
  const review = latestReview(record);
  return review ? REVIEW_LABELS[review.status] : "Not reviewed";
}

export const REVIEW_LABELS: Record<
  BenchmarkRecord["reviews"][number]["status"],
  string
> = {
  correct: "Reviewed: correct",
  partiallyCorrect: "Reviewed: partly correct",
  incorrect: "Reviewed: incorrect",
};

/** Only a review of this exact output counts. */
export function latestReview(record: BenchmarkRecord) {
  return record.reviews
    .filter((review) => review.outputSha256 === record.outputSha256)
    .sort((a, b) => b.reviewedAt - a.reviewedAt)[0];
}

/** "First request" or "Repeat": one pair per case, not a stable estimate. */
export function requestLabel(record: BenchmarkRecord): string {
  return record.cold ? "First request" : "Repeat";
}

/** "1.2 s"; startup is separate and only shown when it was recorded. */
export function durationLabel(record: BenchmarkRecord): string {
  const task = `${(record.taskDurationMs / 1000).toFixed(1)} s`;
  const startup = record.timing.processStartMs;
  return record.cold && startup !== null
    ? `${task} (+${(startup / 1000).toFixed(1)} s startup)`
    : task;
}

const PROCESS_LABELS: Record<BenchmarkMemory["process"], string> = {
  "llama-server": "model server",
  folio: "Folio",
};

/**
 * Each measured process's own peak, never the device's RAM. A peak that
 * wasn't read says so, with its reason.
 */
export function memoryLabels(record: BenchmarkRecord): string[] {
  if (!record.memory.length) return ["Not measured"];
  return record.memory.map((memory) => {
    const process = PROCESS_LABELS[memory.process];
    return memory.peakBytes === null
      ? `${process}: not measured${memory.unavailableReason ? ` (${memory.unavailableReason})` : ""}`
      : `${process}: ${exactSize(memory.peakBytes).split(" (")[0]} peak`;
  });
}

/**
 * Whether the run really stayed on the CPU, from the server's own output.
 * In-process search rows have no server, and "can't tell" is never "yes".
 */
export function cpuLabel(record: BenchmarkRecord): string {
  const backend = record.runtimeDetail.backend;
  if (!backend) return "In-process (no model server)";
  if (backend.gpuOffload === "runtimeDefault") return "Runtime's default";
  if (backend.cpuOnlyVerified === true) return "CPU only (confirmed)";
  if (backend.cpuOnlyVerified === false) {
    return backend.gpuLayersOffloaded !== undefined
      ? `GPU used (${backend.gpuLayersOffloaded} layers)`
      : "GPU used";
  }
  return "CPU only requested; not confirmed";
}

/** "llama.cpp b11524": the runtime that ran this row. */
export function runtimeLabel(record: BenchmarkRecord): string {
  return `${record.runtimeDetail.name} ${record.runtimeDetail.version}`;
}

/** The context and sampling budget the row ran with. */
export function budgetLabels(record: BenchmarkRecord): string[] {
  const { conditions } = record;
  return [
    `${conditions.nCtx} tokens context, up to ${conditions.maxOutputTokens} written, ${conditions.maxPassages} passages`,
    `Temperature ${conditions.temperature}, seed ${conditions.seed}, ${conditions.threads} threads`,
  ];
}

/**
 * What a whole run shares, as label/value pairs: the computer and the test
 * set. Installed RAM is capacity, never usage.
 */
export function runConditionRows(
  run: Pick<BenchmarkRunSummary, "host" | "suite" | "indexBuildMs">,
): { label: string; value: string }[] {
  const { host, suite } = run;
  const ram =
    host.installedRamBytes === null
      ? "installed RAM unknown"
      : `${(host.installedRamBytes / 1024 ** 3).toFixed(1)} GB RAM installed`;
  const rows = [
    {
      label: "Computer",
      value: [
        host.os,
        host.osVersion,
        host.arch,
        host.cpuBrand,
        `${host.logicalCpus} logical CPUs`,
        ram,
      ]
        .filter(Boolean)
        .join(" · "),
    },
    {
      label: "Test set",
      value: `${suite.id}${suite.frozen ? "" : " (development set, not frozen)"}`,
    },
    {
      label: "Not controlled",
      value:
        "The system's file cache, and other work Folio was doing at the time",
    },
  ];
  if (run.indexBuildMs !== null) {
    rows.push({
      label: "Building test passages",
      value: `${(run.indexBuildMs / 1000).toFixed(1)} s`,
    });
  }
  return rows;
}

/** Records kept per task, in the order they ran. */
export function recordsByTask(records: BenchmarkRecord[]) {
  const ordered = [...records].sort((a, b) => a.createdAt - b.createdAt);
  return (["retrieval", "interpretation", "summary", "edit"] as const).map(
    (task) => ({
      task,
      records: ordered.filter((record) => record.task === task),
    }),
  );
}

/** "Qwen3-0.6B · Q4_K_M", marked when it's an evaluation-only candidate. */
export function recordModelLabel(record: BenchmarkRecord): string {
  const name = modelName(record.model);
  return record.model.evaluationOnly ? `${name} (evaluation only)` : name;
}
