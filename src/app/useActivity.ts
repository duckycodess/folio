import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { listHistory, previewUndo, undoPlan } from "../adapters/actions";
import { groupHistory, type ActivityBatch } from "../domain/activity";
import type { UndoPreflight, UndoReport } from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import { actionReducer, IDLE, type ActionState } from "./actionState";
import type { WorkspaceState } from "./useWorkspace";

export interface ActivityState {
  /** `idle` without a folder; history is only read for an open folder. */
  status: "idle" | "loading" | "ready" | "failed";
  batches: ActivityBatch[];
  /** True when the history reached its limit, so older changes aren't shown. */
  truncated: boolean;
  failure: FolioError | null;
  reload: () => void;
  /** The batch whose Undo is being previewed or confirmed. */
  undoTarget: ActivityBatch | null;
  undoPreview: UndoPreflight | null;
  undoBusy: boolean;
  undoFailure: FolioError | null;
  /** True when the failed Undo had already reversed some files. */
  undoPartial: boolean;
  startUndo: (batch: ActivityBatch) => void;
  confirmUndo: () => void;
  closeUndo: () => void;
  /** Settles only from the native Undo report. */
  undoResult: ActionState<UndoReport>;
  dismissUndoResult: () => void;
}

const HISTORY_LIMIT = 500;

/**
 * `onFilesChanged` runs after an Undo changed files, so views built from the
 * index (Related, Graph) re-read it; Activity reloads its own history.
 */
export function useActivity(
  workspace: WorkspaceState,
  onFilesChanged: () => void = () => {},
): ActivityState {
  const folderId = workspace.workspace?.id;
  const [status, setStatus] = useState<ActivityState["status"]>("idle");
  const [batches, setBatches] = useState<ActivityBatch[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [failure, setFailure] = useState<FolioError | null>(null);
  const [generation, setGeneration] = useState(0);
  const [undoTarget, setUndoTarget] = useState<ActivityBatch | null>(null);
  const [undoPreview, setUndoPreview] = useState<UndoPreflight | null>(null);
  const [undoBusy, setUndoBusy] = useState(false);
  const [undoFailure, setUndoFailure] = useState<FolioError | null>(null);
  const [undoPartial, setUndoPartial] = useState(false);
  const [undoResult, dispatch] = useReducer(actionReducer<UndoReport>, IDLE);
  const request = useRef(0);

  // Another folder: nothing from the previous folder's Undo carries over.
  useEffect(() => {
    request.current++;
    setUndoTarget(null);
    setUndoPreview(null);
    setUndoFailure(null);
    setUndoPartial(false);
    setUndoBusy(false);
    dispatch({ type: "reset" });
  }, [folderId]);

  useEffect(() => {
    if (!folderId) {
      setStatus("idle");
      setBatches([]);
      return;
    }
    let active = true;
    setStatus("loading");
    setFailure(null);
    listHistory(folderId, HISTORY_LIMIT)
      .then((entries) => {
        if (!active) return;
        setBatches(groupHistory(entries));
        setTruncated(entries.length >= HISTORY_LIMIT);
        setStatus("ready");
      })
      .catch((cause) => {
        if (!active) return;
        setFailure(toFolioError(cause));
        setStatus("failed");
      });
    return () => {
      active = false;
    };
  }, [folderId, generation]);

  const reload = useCallback(() => setGeneration((value) => value + 1), []);

  async function startUndo(batch: ActivityBatch) {
    if (!folderId) return;
    setUndoTarget(batch);
    setUndoPreview(null);
    setUndoFailure(null);
    setUndoPartial(false);
    setUndoBusy(true);
    try {
      setUndoPreview(await previewUndo(folderId, batch.planId));
    } catch (cause) {
      setUndoFailure(toFolioError(cause));
    } finally {
      setUndoBusy(false);
    }
  }

  async function confirmUndo() {
    if (!folderId || !undoPreview?.undoable) return;
    const current = ++request.current;
    setUndoBusy(true);
    setUndoFailure(null);
    dispatch({ type: "start", request: current });
    try {
      const report = await undoPlan(folderId, undoPreview);
      if (report.error) {
        // Stopped partway: what was reversed stays reversed (partialUndo).
        dispatch({
          type: "nativeFailed",
          request: current,
          error: toFolioError(report.error),
        });
        setUndoFailure(toFolioError(report.error));
        setUndoPartial(report.undoneEntryIds.length > 0);
      } else {
        dispatch({ type: "nativeSucceeded", request: current, result: report });
        setUndoTarget(null);
        setUndoPreview(null);
      }
      reload();
      onFilesChanged();
      await workspace.refreshFolder();
    } catch (cause) {
      // Refused before any file changed.
      const error = toFolioError(cause);
      dispatch({ type: "nativeFailed", request: current, error });
      setUndoFailure(error);
      setUndoPartial(false);
    } finally {
      setUndoBusy(false);
    }
  }

  return {
    status,
    batches,
    truncated,
    failure,
    reload,
    undoTarget,
    undoPreview,
    undoBusy,
    undoFailure,
    undoPartial,
    startUndo: (batch) => void startUndo(batch),
    confirmUndo: () => void confirmUndo(),
    closeUndo: () => {
      setUndoTarget(null);
      setUndoPreview(null);
      setUndoFailure(null);
    },
    undoResult,
    dismissUndoResult: () => dispatch({ type: "reset" }),
  };
}
