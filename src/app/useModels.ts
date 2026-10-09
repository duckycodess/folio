import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  cancelInstall,
  installModel,
  installRuntime,
  isAvailable,
  listModels,
  modelSetup,
  onInstallProgress,
  removeModel,
  runtimeStatus,
  selectModel,
  verifyModel,
} from "../adapters/models";
import type {
  DownloadProgress,
  ModelDescriptor,
  ModelInstallState,
  ModelRole,
  ModelSetup,
  RuntimeStatus,
} from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import {
  installSteps,
  modelGroups,
  modelName,
  type InstallStep,
  type ModelGroup,
} from "./models";

export type ModelsLoad = "loading" | "ready" | "desktopOnly" | "failed";

export interface ModelInstall {
  modelId: string;
  step: InstallStep;
  progress: DownloadProgress | null;
  cancelling: boolean;
}

export interface ModelsController {
  load: ModelsLoad;
  groups: ModelGroup[];
  setup: ModelSetup | null;
  runtime: RuntimeStatus | null;
  /** The one download running now; the native core allows only one. */
  installing: ModelInstall | null;
  /** The model whose selection or removal is being saved. */
  saving: string | null;
  error: FolioError | null;
  /** Shown after a cancelled download, which is not an error. */
  notice: string | null;
  install: (descriptor: ModelDescriptor) => void;
  cancel: () => void;
  remove: (descriptor: ModelDescriptor) => void;
  select: (role: ModelRole, modelId: string) => void;
  reload: () => void;
  dismiss: () => void;
}

/**
 * The running download, kept outside Model Lab: the native download goes on
 * when the user leaves the page, so its progress and Cancel must still be
 * there when they come back. `finished` counts downloads that ended, so a
 * page opened meanwhile reads the model store again.
 */
let activeInstall: ModelInstall | null = null;
let finishedInstalls = 0;
const installListeners = new Set<() => void>();

function setInstalling(
  next:
    | ModelInstall
    | null
    | ((current: ModelInstall | null) => ModelInstall | null),
) {
  const value = typeof next === "function" ? next(activeInstall) : next;
  if (activeInstall !== null && value === null) finishedInstalls += 1;
  activeInstall = value;
  installListeners.forEach((listener) => listener());
}

function subscribeInstall(listener: () => void) {
  installListeners.add(listener);
  return () => installListeners.delete(listener);
}

/**
 * Model setup through the native model store. Downloads start only from a
 * button press, use the pinned manifest's sizes and hashes, and can be
 * cancelled. Nothing here touches the user's files.
 */
export function useModels(): ModelsController {
  const [load, setLoad] = useState<ModelsLoad>(() =>
    isAvailable() ? "loading" : "desktopOnly",
  );
  const [descriptors, setDescriptors] = useState<ModelDescriptor[]>([]);
  const [states, setStates] = useState<Record<string, ModelInstallState>>({});
  const [setup, setSetup] = useState<ModelSetup | null>(null);
  const [runtime, setRuntime] = useState<RuntimeStatus | null>(null);
  const installing = useSyncExternalStore(
    subscribeInstall,
    () => activeInstall,
  );
  const finished = useSyncExternalStore(
    subscribeInstall,
    () => finishedInstalls,
  );
  const [saving, setSaving] = useState<string | null>(null);
  const [error, setError] = useState<FolioError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const verifyAll = useCallback(async (models: ModelDescriptor[]) => {
    // Each answer appears as it arrives; a slow hash check doesn't hold up
    // the others.
    await Promise.all(
      models.map((model) =>
        verifyModel(model.id)
          .then((state) => {
            if (mounted.current)
              setStates((current) => ({ ...current, [model.id]: state }));
          })
          .catch((cause) => {
            if (mounted.current)
              setStates((current) => ({
                ...current,
                [model.id]: {
                  id: model.id,
                  status: "corrupt",
                  error: toFolioError(cause).toPayload(),
                },
              }));
          }),
      ),
    );
  }, []);

  const refreshSetup = useCallback(async () => {
    const next = await modelSetup();
    const status = await runtimeStatus(next.hostRuntimeId);
    if (!mounted.current) return;
    setSetup(next);
    setRuntime(status);
  }, []);

  useEffect(() => {
    if (!isAvailable()) return;
    let active = true;
    setLoad("loading");
    Promise.all([listModels(), refreshSetup()])
      .then(([models]) => {
        if (!active) return;
        setDescriptors(models);
        setLoad("ready");
        void verifyAll(models);
      })
      .catch((cause) => {
        if (!active) return;
        setError(toFolioError(cause));
        setLoad("failed");
      });
    return () => {
      active = false;
    };
  }, [generation, refreshSetup, verifyAll]);

  // A download started on an earlier visit to Model Lab can end while this
  // one is open; read the model store again so its result shows. A download
  // started here refreshes itself.
  const seenFinished = useRef(finished);
  const installedHere = useRef(false);
  useEffect(() => {
    if (finished === seenFinished.current) return;
    seenFinished.current = finished;
    if (installedHere.current) installedHere.current = false;
    else setGeneration((value) => value + 1);
  }, [finished]);

  async function install(descriptor: ModelDescriptor) {
    if (activeInstall || !setup) return;
    installedHere.current = true;
    setError(null);
    setNotice(null);
    const steps = installSteps(descriptor, runtime);
    const stop = await onInstallProgress((progress) =>
      setInstalling((current) => current && { ...current, progress }),
    );
    try {
      for (const step of steps) {
        setInstalling((current) => ({
          modelId: descriptor.id,
          step,
          progress: null,
          cancelling: current?.cancelling ?? false,
        }));
        if (step === "runtime")
          setRuntime(await installRuntime(setup.hostRuntimeId));
        else {
          const state = await installModel(descriptor.id);
          setStates((current) => ({ ...current, [descriptor.id]: state }));
        }
      }
      // The first model set up for a job is the one Folio uses for it.
      const chosen =
        descriptor.role === "embedding"
          ? setup.selectedEmbedding
          : setup.selectedGeneration;
      if (!chosen) await selectModel(descriptor.role, descriptor.id);
      await refreshSetup();
    } catch (cause) {
      const failure = toFolioError(cause);
      if (failure.code === "cancelled")
        setNotice(
          `Download cancelled. ${modelName(descriptor)} wasn't set up.`,
        );
      else setError(failure);
      void verifyAll([descriptor]);
      void refreshSetup().catch(() => undefined);
    } finally {
      stop();
      // Always cleared, even if Model Lab was left meanwhile.
      setInstalling(null);
    }
  }

  async function save(modelId: string, work: () => Promise<unknown>) {
    if (installing || saving) return;
    setError(null);
    setNotice(null);
    setSaving(modelId);
    try {
      await work();
    } catch (cause) {
      setError(toFolioError(cause));
    } finally {
      await refreshSetup().catch(() => undefined);
      if (mounted.current) setSaving(null);
    }
  }

  return {
    load,
    groups: modelGroups(descriptors, states, setup),
    setup,
    runtime,
    installing,
    saving,
    error,
    notice,
    install: (descriptor) => void install(descriptor),
    cancel: () => {
      setInstalling((current) => current && { ...current, cancelling: true });
      void cancelInstall().catch((cause) => setError(toFolioError(cause)));
    },
    remove: (descriptor) =>
      void save(descriptor.id, async () => {
        await removeModel(descriptor.id);
        await verifyAll([descriptor]);
      }),
    select: (role, modelId) =>
      void save(modelId, () => selectModel(role, modelId)),
    reload: () => {
      setError(null);
      setGeneration((value) => value + 1);
    },
    dismiss: () => {
      setError(null);
      setNotice(null);
    },
  };
}
