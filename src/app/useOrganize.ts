import { useEffect, useReducer, useRef, useState } from "react";
import {
  applyPlan,
  approvePlan,
  listHistory,
  organizationSuggestions,
  suggestFileChanges,
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
  NewPlanSource,
  UndoPreflight,
  UndoReport,
} from "../domain/contracts";
import { folioError, toFolioError, type FolioError } from "../domain/errors";
import { stopSuggestions } from "../adapters/collections";
import {
  chosenOperations,
  ORGANIZE_START,
  organizeFlow,
  type OrganizeState,
} from "./organizeFlow";
import { relocateOperation } from "./fileActions";
import type { WorkspaceState } from "./useWorkspace";

export interface UndoState {
  preview: UndoPreflight | null;
  report: UndoReport | null;
  busy: boolean;
  error: FolioError | null;
}

export interface OrganizeController {
  state: OrganizeState;
  /**
   * Re-index the folder (with progress), then ask for suggestions: for the
   * whole folder, or only for the members of the target collection. Resolves
   * true once suggestions are on screen.
   */
  analyze: () => Promise<boolean>;
  /** The collection being analyzed, or `null` for the whole folder. */
  target: string | null;
  setTarget: (collectionId: string | null) => void;
  /** Stops indexing; work already indexed is kept. */
  cancelAnalyze: () => void;
  /** Chooses or drops one suggestion, by `suggestionKey`. */
  toggle: (key: string) => void;
  /**
   * Asks the local models for renames and moves (for the target collection's
   * files, or the whole folder). Resolves once they arrive, fail or stop.
   */
  suggestWithModel: () => Promise<void>;
  /**
   * Stops the local models' suggestions natively, and only theirs. The names
   * already written and the moves found so far still arrive.
   */
  stopAssist: () => void;
  previewChosen: () => void;
  /** Renames a file in place, or moves it to another folder, via an exact plan. */
  previewRelocate: (
    document: DocumentRecord,
    change: { name: string } | { folder: string },
  ) => void;
  /**
   * Builds operations (for example by asking the native core for an exact
   * passage edit), then previews them. A failure while building is shown
   * like a refused preview.
   */
  previewFrom: (
    build: (workspaceId: string) => Promise<FileOperation[]>,
  ) => void;
  /** Creates one new Markdown file, via an exact plan. */
  previewCreate: (relativePath: string, content: string) => void;
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

export function useOrganize(
  workspace: WorkspaceState,
  /**
   * Where in Folio these changes start (Activity shows it). Part of each
   * plan's digest, so it can't be changed after approval.
   */
  source: NewPlanSource,
  /** Called after Folio changed files, so other views re-read the index. */
  onFilesChanged: () => void = () => {},
): OrganizeController {
  const [state, dispatch] = useReducer(organizeFlow, ORGANIZE_START);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [undo, setUndo] = useState<UndoState>(NO_UNDO);
  const [target, setTarget] = useState<string | null>(null);
  const next = useRef(0);
  // The local models' suggestions run beside the plan flow, with their own count.
  const assistNext = useRef(0);
  const folderId = workspace.workspace?.id;

  // A different folder starts the flow over.
  useEffect(() => {
    dispatch({ type: "reset", request: ++next.current });
    setHistory([]);
    setUndo(NO_UNDO);
    setTarget(null);
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

  async function analyze(): Promise<boolean> {
    const request = ++next.current;
    dispatch({ type: "analyzeStarted", request });
    if (!folderId) {
      noFolder(request);
      return false;
    }
    const stop = await onIndexProgress((progress) => {
      if (progress.workspaceId === folderId)
        dispatch({ type: "progress", request, progress });
    }).catch(() => undefined);
    try {
      const scan = await scanWorkspace(folderId);
      if (scan.cancelled) {
        dispatch({ type: "analyzeCancelled", request });
        return false;
      }
      const suggestions = await organizationSuggestions(
        folderId,
        target ?? undefined,
      );
      dispatch({ type: "analyzed", request, suggestions });
      return next.current === request;
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
      return false;
    } finally {
      stop?.();
    }
  }

  function cancelAnalyze() {
    dispatch({ type: "stopAnalyze", request: ++next.current });
    void cancelIndexing().catch(() => undefined);
  }

  async function suggestWithModel() {
    const request = ++assistNext.current;
    dispatch({ type: "assistStarted", request });
    if (!folderId) return;
    try {
      const result = await suggestFileChanges(folderId, target ?? undefined);
      dispatch({ type: "assisted", request, result });
    } catch (cause) {
      dispatch({ type: "assistFailed", request, error: toFolioError(cause) });
    }
  }

  async function preview(operations: FileOperation[]) {
    const request = ++next.current;
    dispatch({ type: "prepareStarted", request, operations });
    if (!folderId) return noFolder(request);
    try {
      const plan = await preparePlan(folderId, source, operations);
      dispatch({ type: "prepared", request, plan });
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  async function previewFrom(
    build: (workspaceId: string) => Promise<FileOperation[]>,
  ) {
    const request = ++next.current;
    dispatch({ type: "prepareStarted", request, operations: [] });
    if (!folderId) return noFolder(request);
    let operations: FileOperation[];
    try {
      operations = await build(folderId);
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
      return;
    }
    // A newer request (or Cancel) since this one started wins.
    if (next.current === request) await preview(operations);
  }

  async function previewRelocate(
    document: DocumentRecord,
    change: { name: string } | { folder: string },
  ) {
    if (!folderId) return;
    // The plan pins the exact revision; read it if the listing didn't.
    try {
      const read = document.contentHash
        ? document
        : await readNativeDocument(folderId, document);
      if (!read.contentHash) throw folioError("internal", "No content hash.");
      await preview([
        relocateOperation({ ...read, contentHash: read.contentHash }, change),
      ]);
    } catch (cause) {
      const request = ++next.current;
      dispatch({ type: "prepareStarted", request, operations: [] });
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  async function approveAndApply() {
    const plan = state.plan;
    // Only from a preview on screen, once: never twice, or after a refusal.
    if (!folderId || !plan || state.stage !== "preview" || state.error) return;
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
    analyze,
    target,
    setTarget: (collectionId) => {
      setTarget(collectionId);
      // Suggestions for another target would be misleading: start over.
      dispatch({ type: "reset", request: ++next.current });
    },
    cancelAnalyze,
    toggle: (key) => dispatch({ type: "toggle", key }),
    suggestWithModel,
    stopAssist: () => {
      if (state.assist.status !== "working") return;
      // The stopped request's reply still lands, with what it already found.
      dispatch({ type: "assistStopped" });
      void stopSuggestions().catch(() => undefined);
    },
    previewChosen: () => {
      const operations = chosenOperations(state);
      if (operations.length) void preview(operations);
    },
    previewRelocate: (document, change) =>
      void previewRelocate(document, change),
    previewFrom: (build) => void previewFrom(build),
    previewCreate: (relativePath, content) =>
      void preview([
        {
          kind: "create",
          destinationRelativePath: relativePath,
          mediaType: "text/markdown",
          content,
          expectedDestination: "absent",
        },
      ]),
    previewAgain: () => {
      if (state.operations.length) void preview(state.operations);
    },
    approveAndApply: () => void approveAndApply(),
    backToSuggestions: () => dispatch({ type: "backToSuggestions" }),
    dismissError: () => dispatch({ type: "dismissError" }),
    done: () => {
      dispatch({ type: "reset", request: ++next.current });
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
