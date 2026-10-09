import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  BenchmarkRecord,
  BenchmarkReview,
  BenchmarkRunSummary,
  LabModel,
  LabProgress,
  LabRunRequest,
} from "../domain/contracts";
import { folioError, toFolioError } from "../domain/errors";

export function isAvailable(): boolean {
  return isTauri();
}

function unavailable(): never {
  throw folioError(
    "modelNotInstalled",
    "Model Lab runs in the Folio desktop app.",
    { component: "runtime", reason: "browserPreview" },
  );
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  // Model Lab shows only what the native core measured. The browser preview
  // has no measurements, so it never shows fixture data in their place.
  if (!isAvailable()) unavailable();
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    throw toFolioError(cause);
  }
}

/** Manifest models with their install state. Selection stays with `select_model`. */
export function labModels(): Promise<LabModel[]> {
  return call("lab_models");
}

/** Starts a run and returns its id; results arrive through `onLabProgress`. */
export function runModelLab(
  request: LabRunRequest,
): Promise<{ runId: string }> {
  return call("run_model_lab", { request });
}

export function cancelModelLab(): Promise<void> {
  return call("cancel_model_lab");
}

export function listLabRuns(): Promise<BenchmarkRunSummary[]> {
  return call("list_lab_runs");
}

export interface LabResultFilter {
  runId?: string;
  modelId?: string;
  task?: BenchmarkRecord["task"];
}

export function listLabResults(
  filter: LabResultFilter = {},
): Promise<BenchmarkRecord[]> {
  return call("list_lab_results", {
    runId: filter.runId,
    modelId: filter.modelId,
    task: filter.task,
  });
}

/**
 * Appends a review. `outputSha256` must be the hash of the output the reviewer
 * read; the native core refuses a review of any other output.
 */
export function recordLabReview(review: {
  id: string;
  outputSha256: string;
  status: BenchmarkReview["status"];
  reviewer: string;
  notes?: string;
}): Promise<BenchmarkRecord> {
  return call("record_lab_review", review);
}

export async function onLabProgress(
  handler: (progress: LabProgress) => void,
): Promise<UnlistenFn> {
  if (!isAvailable()) unavailable();
  return listen<LabProgress>("folio://lab-progress", (event) =>
    handler(event.payload),
  );
}
