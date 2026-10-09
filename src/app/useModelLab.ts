import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  cancelModelLab,
  isAvailable,
  labModels,
  listLabResults,
  listLabRuns,
  onLabProgress,
  runModelLab,
} from "../adapters/modelLab";
import type {
  BenchmarkRecord,
  BenchmarkRunSummary,
  LabModel,
  LabProgress,
} from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import {
  isRunEnd,
  labChoices,
  nextSelection,
  runRequest,
  sortRuns,
  toggleGeneration,
  type LabChoices,
  type LabSelection,
} from "./modelLab";

export type LabLoad = "loading" | "ready" | "desktopOnly" | "failed";

export interface ActiveRun {
  /** Null between Start and the native core's answer. */
  runId: string | null;
  progress: LabProgress | null;
  cancelling: boolean;
}

export interface ModelLabController {
  load: LabLoad;
  models: LabModel[];
  choices: LabChoices;
  selection: LabSelection;
  setEmbedding: (modelId: string) => void;
  toggleGeneration: (modelId: string) => void;
  /** Why Start is unavailable, or null when it can run. */
  blocked: string | null;
  running: ActiveRun | null;
  start: () => void;
  cancel: () => void;
  runs: BenchmarkRunSummary[];
  runId: string | null;
  chooseRun: (runId: string) => void;
  records: BenchmarkRecord[];
  recordsLoading: boolean;
  error: FolioError | null;
  /** How the last run ended, until dismissed. */
  ended: LabProgress | null;
  reload: () => void;
  dismiss: () => void;
}

/**
 * The running comparison, kept outside the page: a run goes on natively when
 * the user leaves Model Lab, so its progress and Stop must still be there when
 * they come back. `finishedRuns` counts runs that ended, so an open page reads
 * the runs and results again.
 */
let activeRun: ActiveRun | null = null;
let lastEnd: LabProgress | null = null;
let finishedRuns = 0;
let progressSubscription: Promise<unknown> | null = null;
const listeners = new Set<() => void>();

function setActiveRun(next: ActiveRun | null) {
  activeRun = next;
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function handleProgress(progress: LabProgress) {
  // Another run's events are ignored; with no run open here, one is adopted.
  if (activeRun?.runId && activeRun.runId !== progress.runId) return;
  if (isRunEnd(progress)) {
    lastEnd = progress;
    finishedRuns += 1;
    setActiveRun(null);
    return;
  }
  setActiveRun({
    runId: progress.runId,
    progress,
    cancelling: activeRun?.cancelling ?? false,
  });
}

/** One listener for the app's lifetime, started on first use. */
function ensureProgressListener() {
  progressSubscription ??= onLabProgress(handleProgress).catch(() => {
    progressSubscription = null;
  });
  return progressSubscription;
}

function useLabRun() {
  const running = useSyncExternalStore(subscribe, () => activeRun);
  const finished = useSyncExternalStore(subscribe, () => finishedRuns);
  const ended = useSyncExternalStore(subscribe, () => lastEnd);
  return { running, finished, ended };
}

/**
 * `installedModels` changes whenever a model is installed, removed or chosen
 * on the same page, so the run's choices are read again.
 */
export function useModelLab(installedModels = ""): ModelLabController {
  const available = isAvailable();
  const [load, setLoad] = useState<LabLoad>(
    available ? "loading" : "desktopOnly",
  );
  const [models, setModels] = useState<LabModel[]>([]);
  const [runs, setRuns] = useState<BenchmarkRunSummary[]>([]);
  const [runId, setRunId] = useState<string | null>(null);
  const [records, setRecords] = useState<BenchmarkRecord[]>([]);
  const [recordsLoading, setRecordsLoading] = useState(false);
  const [selection, setSelection] = useState<LabSelection>({
    embeddingModelId: null,
    generationModelIds: [],
  });
  const [error, setError] = useState<FolioError | null>(null);
  const [attempt, setAttempt] = useState(0);
  const { running, finished, ended } = useLabRun();
  const seenFinished = useRef(finished);
  const choices = labChoices(models);

  // Models and runs: on open, on Retry, and whenever a run ends.
  useEffect(() => {
    if (!available) return;
    let active = true;
    void ensureProgressListener();
    Promise.all([labModels(), listLabRuns()])
      .then(([nextModels, nextRuns]) => {
        if (!active) return;
        const sorted = sortRuns(nextRuns);
        setModels(nextModels);
        setRuns(sorted);
        setSelection((current) =>
          nextSelection(current, labChoices(nextModels)),
        );
        // After a run ends, its results are the ones shown.
        const justEnded = seenFinished.current !== finished;
        seenFinished.current = finished;
        setRunId((current) =>
          !justEnded && current && sorted.some((run) => run.runId === current)
            ? current
            : (sorted[0]?.runId ?? null),
        );
        const stillRunning = sorted.find((run) => run.status === "running");
        if (stillRunning && !activeRun) {
          setActiveRun({
            runId: stillRunning.runId,
            progress: null,
            cancelling: false,
          });
        }
        setLoad("ready");
      })
      .catch((cause: unknown) => {
        if (!active) return;
        setError(toFolioError(cause));
        setLoad("failed");
      });
    return () => {
      active = false;
    };
  }, [available, attempt, finished, installedModels]);

  // The chosen run's records. They outlive the models they measured.
  useEffect(() => {
    if (!available || !runId) {
      setRecords([]);
      return;
    }
    let active = true;
    setRecordsLoading(true);
    listLabResults({ runId })
      .then((next) => {
        if (active) setRecords(next);
      })
      .catch((cause: unknown) => {
        if (active) setError(toFolioError(cause));
      })
      .finally(() => {
        if (active) setRecordsLoading(false);
      });
    return () => {
      active = false;
    };
  }, [available, runId, finished, attempt]);

  const planned = runRequest(selection, choices);

  const start = useCallback(() => {
    if (!("request" in planned) || activeRun) return;
    setError(null);
    lastEnd = null;
    setActiveRun({ runId: null, progress: null, cancelling: false });
    void ensureProgressListener()
      .then(() => runModelLab(planned.request))
      .then(({ runId: started }) => {
        // An end event can arrive before this answer; then the run is over.
        if (activeRun && activeRun.runId === null) {
          setActiveRun({ ...activeRun, runId: started });
        }
        // The results stay on a recorded run; the new one is listed, and
        // shown, once it ends.
      })
      .catch((cause: unknown) => {
        setActiveRun(null);
        setError(toFolioError(cause));
      });
  }, [planned]);

  const cancel = useCallback(() => {
    if (!activeRun || activeRun.cancelling) return;
    setActiveRun({ ...activeRun, cancelling: true });
    cancelModelLab().catch((cause: unknown) => {
      if (activeRun) setActiveRun({ ...activeRun, cancelling: false });
      setError(toFolioError(cause));
    });
  }, []);

  return {
    load,
    models,
    choices,
    selection,
    setEmbedding: (modelId) =>
      setSelection((current) => ({ ...current, embeddingModelId: modelId })),
    toggleGeneration: (modelId) =>
      setSelection((current) => toggleGeneration(current, modelId)),
    blocked: "blocked" in planned ? planned.blocked : null,
    running,
    start,
    cancel,
    runs,
    runId,
    chooseRun: setRunId,
    records,
    recordsLoading,
    error,
    ended,
    reload: () => {
      setError(null);
      setLoad("loading");
      setAttempt((value) => value + 1);
    },
    dismiss: () => {
      setError(null);
      lastEnd = null;
      listeners.forEach((listener) => listener());
    },
  };
}
