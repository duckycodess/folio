import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  cancelLocalAiRefresh,
  isAvailable,
  onAiRefreshProgress,
  refreshLocalAiIndex,
  relationshipCoverage,
} from "../adapters/ai";
import { shouldAutoRefresh } from "../domain/aiCoverage";
import type {
  AiRefreshPhase,
  AiRelationshipCoverage,
} from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";

export interface AiIndexState {
  /** `null` until read, and in the browser preview, which has none. */
  coverage: AiRelationshipCoverage | null;
  refreshing: boolean;
  phase: AiRefreshPhase | null;
  /** Pairs finished in the running refresh. */
  pairsCompleted: number;
  failure: FolioError | null;
  /** Starts (or continues) the refresh; ignored while one runs or no model is ready. */
  refresh: () => void;
  stop: () => void;
}

const IDLE: AiIndexState = {
  coverage: null,
  refreshing: false,
  phase: null,
  pairsCompleted: 0,
  failure: null,
  refresh: () => {},
  stop: () => {},
};

const AiIndexContext = createContext<AiIndexState | null>(null);
export const AiIndexProvider = AiIndexContext.Provider;

/** The shell's AI index state, or an idle one outside it (tests, previews). */
export function useAiIndexState(): AiIndexState {
  return useContext(AiIndexContext) ?? IDLE;
}

/**
 * Local AI refresh for the open folder (#46). Coverage is read from the native
 * core, which resolves the active search model's space on every read, and is
 * dropped and re-read when the selected search model changes. A refresh starts
 * after Local Sync, an applied change or Undo, but only when the search model
 * is ready; it shows progress, has a Stop, and keeps completed work.
 */
export function useAiIndex(
  folderId: string | undefined,
  searchModel: string | null,
  /** Called when the displayed connections should be read again. */
  onChanged: () => void,
): AiIndexState {
  const [coverage, setCoverage] = useState<AiRelationshipCoverage | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [phase, setPhase] = useState<AiRefreshPhase | null>(null);
  const [pairsCompleted, setPairsCompleted] = useState(0);
  const [failure, setFailure] = useState<FolioError | null>(null);
  const running = useRef(false);
  const latest = useRef({ folderId, searchModel, onChanged });
  latest.current = { folderId, searchModel, onChanged };
  // A refresh that finishes after the folder or model changed must not land.
  const generation = useRef(0);

  // Drop and re-read on a folder or search-model change.
  useEffect(() => {
    generation.current += 1;
    const mine = generation.current;
    setCoverage(null);
    setFailure(null);
    if (!folderId || !isAvailable()) return;
    relationshipCoverage(folderId)
      .then((read) => {
        if (generation.current === mine) setCoverage(read);
      })
      .catch(() => {
        // Coverage is a qualifier; without it no claim is made either way.
      });
    latest.current.onChanged();
  }, [folderId, searchModel]);

  const refresh = useCallback(() => {
    const {
      folderId: folder,
      searchModel: model,
      onChanged: changed,
    } = latest.current;
    if (
      !folder ||
      !isAvailable() ||
      !shouldAutoRefresh({
        searchModelReady: model !== null,
        refreshing: running.current,
        folderOpen: true,
      })
    )
      return;
    running.current = true;
    const mine = generation.current;
    setRefreshing(true);
    setFailure(null);
    setPairsCompleted(0);
    let unlisten: (() => void) | undefined;
    // The refresh can settle before the listener is attached (a quick refusal);
    // then the listener is removed as soon as it arrives.
    let done = false;
    onAiRefreshProgress((progress) => {
      if (progress.workspaceId !== folder || generation.current !== mine)
        return;
      setPhase(progress.phase);
      setPairsCompleted(progress.pairsCompleted);
    })
      .then((stop) => {
        if (done) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Progress is a nicety; the result is what counts.
      });
    refreshLocalAiIndex(folder)
      .then((result) => {
        if (generation.current === mine) setCoverage(result.coverage);
      })
      .catch((cause) => {
        const error = toFolioError(cause);
        // No ready model or a busy provider isn't a failure to report here.
        if (
          generation.current === mine &&
          error.code !== "modelNotInstalled" &&
          error.code !== "providerBusy" &&
          error.code !== "cancelled"
        )
          setFailure(error);
      })
      .finally(() => {
        done = true;
        unlisten?.();
        running.current = false;
        // Only one refresh runs at a time, so its end always clears the
        // in-progress state, even after a folder or model change; what it
        // found applies only to the folder and model it ran for.
        setRefreshing(false);
        setPhase(null);
        if (generation.current === mine) {
          changed();
          // Read what is true now, including after a Stop or a model change.
          relationshipCoverage(folder)
            .then((read) => {
              if (generation.current === mine) setCoverage(read);
            })
            .catch(() => {});
        }
      });
  }, []);

  const stop = useCallback(() => {
    void cancelLocalAiRefresh().catch(() => {});
  }, []);

  return {
    coverage,
    refreshing,
    phase,
    pairsCompleted,
    failure,
    refresh,
    stop,
  };
}
