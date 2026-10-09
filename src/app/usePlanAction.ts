import { useEffect, useReducer, useRef } from "react";
import {
  applyPlan,
  approvePlan,
  preparePlan,
  previewUndo,
  undoPlan,
} from "../adapters/actions";
import { readNativeDocument } from "../adapters/workspace";
import type {
  DocumentRecord,
  FileOperation,
  ImpactCandidate,
} from "../domain/contracts";
import { folioError, toFolioError } from "../domain/errors";
import {
  PLAN_ACTION_START,
  planAction,
  type PlanActionState,
} from "./planAction";
import type { WorkspaceState } from "./useWorkspace";

/** A document whose exact revision is known, so an operation can pin it. */
export type ReadDocument = DocumentRecord & { contentHash: string };

export interface PlanActionController {
  state: PlanActionState;
  /**
   * Asks the native core for an exact plan. Without `impacts`, it computes
   * Ripple candidates from each edit's diff. Nothing is written.
   */
  prepare: (operations: FileOperation[], impacts?: ImpactCandidate[]) => void;
  /**
   * Like `prepare`, for an operation on one document: reads it first when its
   * revision isn't known yet. "Preview again" always reads it afresh.
   */
  prepareFor: (
    document: DocumentRecord,
    toOperations: (read: ReadDocument) => FileOperation[],
  ) => void;
  /** Repeats the last preview, for a fresh native plan. */
  previewAgain: () => void;
  /** Approves exactly the plan on screen, then applies it. */
  approveAndApply: () => void;
  previewUndo: () => void;
  /** Reverses exactly the entries of the Undo preflight on screen. */
  confirmUndo: () => void;
  closeUndo: () => void;
  dismissError: () => void;
  reset: () => void;
}

type Source = (fresh: boolean) => Promise<{
  operations: FileOperation[];
  impacts?: ImpactCandidate[];
}>;

/**
 * Preview → approve → apply → result → Undo for one plan at a time, against
 * the open folder. Shared by the file action dialogs and Ask & Act.
 */
export function usePlanAction(
  workspace: WorkspaceState,
  /** Called after Folio changed files, so other views re-read the index. */
  onFilesChanged: () => void = () => {},
): PlanActionController {
  const [state, dispatch] = useReducer(planAction, PLAN_ACTION_START);
  const next = useRef(0);
  const source = useRef<Source | null>(null);
  const folderId = workspace.workspace?.id;

  // A different folder starts over; its plans belong to the old one.
  useEffect(() => {
    dispatch({ type: "reset", request: ++next.current });
    source.current = null;
  }, [folderId]);

  async function preview(from: Source, fresh: boolean) {
    source.current = from;
    const request = ++next.current;
    dispatch({ type: "prepareStarted", request });
    try {
      if (!folderId)
        throw folioError(
          "workspaceUnavailable",
          "Add a folder before changing its files.",
        );
      const { operations, impacts } = await from(fresh);
      const plan = await preparePlan(folderId, operations, impacts);
      dispatch({ type: "prepared", request, plan });
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  async function approveAndApply() {
    const plan = state.plan;
    if (!folderId || !plan || state.stage !== "preview" || state.error) return;
    const request = ++next.current;
    dispatch({ type: "applyStarted", request });
    try {
      // The approval echoes the digest of the plan on screen; the native core
      // refuses any other.
      await approvePlan(folderId, plan);
      const report = await applyPlan(folderId, plan);
      dispatch({ type: "applied", request, report });
      onFilesChanged();
      await workspace.refreshFolder();
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  async function startUndoPreview() {
    const plan = state.plan;
    if (!folderId || !plan) return;
    const request = ++next.current;
    dispatch({ type: "undoPreviewStarted", request });
    try {
      const preflight = await previewUndo(folderId, plan.id);
      dispatch({ type: "undoPreviewed", request, preflight });
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  async function confirmUndo() {
    const preflight = state.undoPreflight;
    if (!folderId || !preflight?.undoable || state.stage !== "undoPreview")
      return;
    const request = ++next.current;
    dispatch({ type: "undoStarted", request });
    try {
      const report = await undoPlan(folderId, preflight);
      dispatch({ type: "undone", request, report });
      onFilesChanged();
      await workspace.refreshFolder();
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  return {
    state,
    prepare: (operations, impacts) =>
      void preview(async () => ({ operations, impacts }), false),
    prepareFor: (document, toOperations) =>
      void preview(async (fresh) => {
        // The plan pins the exact revision; read it if the listing didn't.
        const read =
          !fresh && document.contentHash
            ? document
            : await readNativeDocument(folderId!, document);
        if (!read.contentHash) throw folioError("internal", "No content hash.");
        return {
          operations: toOperations({ ...read, contentHash: read.contentHash }),
        };
      }, false),
    previewAgain: () => {
      if (source.current) void preview(source.current, true);
    },
    approveAndApply: () => void approveAndApply(),
    previewUndo: () => void startUndoPreview(),
    confirmUndo: () => void confirmUndo(),
    closeUndo: () => dispatch({ type: "closeUndo", request: ++next.current }),
    dismissError: () => dispatch({ type: "dismissError" }),
    reset: () => {
      source.current = null;
      dispatch({ type: "reset", request: ++next.current });
    },
  };
}
