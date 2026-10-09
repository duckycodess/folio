import { useEffect, useReducer, useRef, useState } from "react";
import {
  applyPlan,
  approvePlan,
  listHistory,
  organizationSuggestions,
  preparePlan,
  previewUndo,
  undoPlan,
} from "../adapters/actions";
import {
  cancelIndexing,
  onIndexProgress,
  readNativeDocument,
  scanWorkspace,
} from "../adapters/workspace";
import type {
  DocumentRecord,
  FileOperation,
  HistoryEntry,
  UndoPreflight,
  UndoReport,
} from "../domain/contracts";
import { folioError, toFolioError, type FolioError } from "../domain/errors";
import {
  ORGANIZE_START,
  organizeFlow,
  type OrganizeState,
} from "./organizeFlow";
import type { WorkspaceState } from "./useWorkspace";

export interface UndoState {
  preview: UndoPreflight | null;
  report: UndoReport | null;
  busy: boolean;
  error: FolioError | null;
}

export interface OrganizeController {
  state: OrganizeState;
  /** Re-index the folder (with progress), then ask for suggestions. */
  analyze: () => void;
  /** Stops indexing; work already indexed is kept. */
  cancelAnalyze: () => void;
  toggle: (documentId: string) => void;
  previewChosen: () => void;
  previewRename: (document: DocumentRecord, newName: string) => void;
  /** Resends the operations last previewed, for a fresh native plan. */
  previewAgain: () => void;
  /** Approves exactly the plan on screen, then applies it. */
  approveAndApply: () => void;
  backToSuggestions: () => void;
  dismissError: () => void;
  done: () => void;
  history: HistoryEntry[];
  undo: UndoState;
  previewUndo: () => void;
  confirmUndo: () => void;
  closeUndo: () => void;
}

const NO_UNDO: UndoState = {
  preview: null,
  report: null,
  busy: false,
  error: null,
};

/** A rename keeps the file in its folder; only the name changes. */
export function renameOperation(
  document: DocumentRecord & { contentHash: string },
  newName: string,
): FileOperation {
  const folder = document.relativePath.split("/").slice(0, -1).join("/");
  return {
    kind: "rename",
    documentId: document.id,
    relativePath: document.relativePath,
    expectedContentHash: document.contentHash,
    destinationRelativePath: folder ? `${folder}/${newName}` : newName,
    expectedDestination: "absent",
  };
}

export function useOrganize(
  workspace: WorkspaceState,
  /** Called after Folio changed files, so other views re-read the index. */
  onFilesChanged: () => void = () => {},
): OrganizeController {
  const [state, dispatch] = useReducer(organizeFlow, ORGANIZE_START);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [undo, setUndo] = useState<UndoState>(NO_UNDO);
  const next = useRef(0);
  const folderId = workspace.workspace?.id;

  // A different folder starts the flow over.
  useEffect(() => {
    dispatch({ type: "reset" });
    setHistory([]);
    setUndo(NO_UNDO);
  }, [folderId]);

  function noFolder(request: number) {
    dispatch({
      type: "failed",
      request,
      error: folioError(
        "workspaceUnavailable",
        "Add a folder before organizing it.",
      ),
    });
  }

  async function analyze() {
    const request = ++next.current;
    dispatch({ type: "analyzeStarted", request });
    if (!folderId) return noFolder(request);
    const stop = await onIndexProgress((progress) => {
      if (progress.workspaceId === folderId)
        dispatch({ type: "progress", request, progress });
    }).catch(() => undefined);
    try {
      const scan = await scanWorkspace(folderId);
      if (scan.cancelled) {
        dispatch({ type: "analyzeCancelled", request });
        return;
      }
      const suggestions = await organizationSuggestions(folderId);
      dispatch({ type: "analyzed", request, suggestions });
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    } finally {
      stop?.();
    }
  }

  function cancelAnalyze() {
    dispatch({ type: "analyzeCancelled", request: next.current });
    void cancelIndexing().catch(() => undefined);
  }

  async function preview(operations: FileOperation[]) {
    const request = ++next.current;
    dispatch({ type: "prepareStarted", request, operations });
    if (!folderId) return noFolder(request);
    try {
      const plan = await preparePlan(folderId, operations);
      dispatch({ type: "prepared", request, plan });
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  async function previewRename(document: DocumentRecord, newName: string) {
    if (!folderId) return;
    // The plan pins the exact revision; read it if the listing didn't.
    try {
      const read = document.contentHash
        ? document
        : await readNativeDocument(folderId, document);
      if (!read.contentHash) throw folioError("internal", "No content hash.");
      await preview([
        renameOperation({ ...read, contentHash: read.contentHash }, newName),
      ]);
    } catch (cause) {
      const request = ++next.current;
      dispatch({ type: "prepareStarted", request, operations: [] });
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  async function approveAndApply() {
    const plan = state.plan;
    if (!folderId || !plan) return;
    const request = ++next.current;
    dispatch({ type: "applyStarted", request });
    try {
      // The approval echoes the digest of the plan on screen; the native
      // core refuses any other.
      await approvePlan(folderId, plan);
      const report = await applyPlan(folderId, plan);
      dispatch({ type: "applied", request, report });
      setUndo(NO_UNDO);
      onFilesChanged();
      await Promise.all([
        workspace.refreshFolder(),
        listHistory(folderId)
          .then((entries) =>
            setHistory(entries.filter((entry) => entry.planId === plan.id)),
          )
          .catch(() => setHistory([])),
      ]);
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  async function startUndoPreview() {
    if (!folderId || !state.plan) return;
    setUndo({ ...NO_UNDO, busy: true });
    try {
      const preview = await previewUndo(folderId, state.plan.id);
      setUndo({ ...NO_UNDO, preview });
    } catch (cause) {
      setUndo({ ...NO_UNDO, error: toFolioError(cause) });
    }
  }

  async function confirmUndo() {
    if (!folderId || !undo.preview?.undoable) return;
    const preview = undo.preview;
    setUndo({ ...undo, busy: true, error: null });
    try {
      const report = await undoPlan(folderId, preview);
      setUndo({ ...NO_UNDO, report });
      onFilesChanged();
      await workspace.refreshFolder();
    } catch (cause) {
      setUndo({ ...undo, busy: false, error: toFolioError(cause) });
    }
  }

  return {
    state,
    analyze: () => void analyze(),
    cancelAnalyze,
    toggle: (documentId) => dispatch({ type: "toggle", documentId }),
    previewChosen: () => {
      const operations = (state.suggestions?.filenames ?? [])
        .filter((item) => state.chosen.includes(item.documentId))
        .map((item) => item.operation);
      if (operations.length) void preview(operations);
    },
    previewRename: (document, newName) => void previewRename(document, newName),
    previewAgain: () => {
      if (state.operations.length) void preview(state.operations);
    },
    approveAndApply: () => void approveAndApply(),
    backToSuggestions: () => dispatch({ type: "backToSuggestions" }),
    dismissError: () => dispatch({ type: "dismissError" }),
    done: () => {
      dispatch({ type: "reset" });
      setUndo(NO_UNDO);
      setHistory([]);
    },
    history,
    undo,
    previewUndo: () => void startUndoPreview(),
    confirmUndo: () => void confirmUndo(),
    closeUndo: () => setUndo(NO_UNDO),
  };
}
