import type {
  ActionPlan,
  ApplyReport,
  UndoPreflight,
  UndoReport,
} from "../domain/contracts";
import type { FolioError } from "../domain/errors";

/**
 * One file action as explicit steps: exact preview → approve and apply →
 * result → Undo. Shared by Edit text, Rename and Move, and by Ask & Act. The
 * plan in `preview` is the one the native core issued; the UI only echoes it
 * back for approval. A result exists only once the native core reported one.
 *
 * Organize keeps its own reducer (`organizeFlow`), which adds analysis and
 * suggestions in front of the same preview and apply steps.
 */
export type PlanActionStage =
  | "idle"
  | "preparing"
  | "preview"
  | "applying"
  | "result"
  | "previewingUndo"
  | "undoPreview"
  | "undoing"
  | "undone";

export interface PlanActionState {
  stage: PlanActionStage;
  /** Only the reply to this request may move the action on. */
  request: number;
  plan: ActionPlan | null;
  report: ApplyReport | null;
  undoPreflight: UndoPreflight | null;
  undoReport: UndoReport | null;
  /** A refusal; the step it came from stays on screen. */
  error: FolioError | null;
}

export type PlanActionEvent =
  | { type: "prepareStarted"; request: number }
  | { type: "prepared"; request: number; plan: ActionPlan }
  | { type: "applyStarted"; request: number }
  | { type: "applied"; request: number; report: ApplyReport }
  | { type: "undoPreviewStarted"; request: number }
  | { type: "undoPreviewed"; request: number; preflight: UndoPreflight }
  | { type: "undoStarted"; request: number }
  | { type: "undone"; request: number; report: UndoReport }
  | { type: "failed"; request: number; error: FolioError }
  /** Back to the result without undoing; a new request drops a pending preflight. */
  | { type: "closeUndo"; request: number }
  | { type: "dismissError" }
  /** Starts over; a new request number drops replies still in flight. */
  | { type: "reset"; request: number };

export const PLAN_ACTION_START: PlanActionState = {
  stage: "idle",
  request: 0,
  plan: null,
  report: null,
  undoPreflight: null,
  undoReport: null,
  error: null,
};

const REPLIES = new Set<PlanActionEvent["type"]>([
  "prepared",
  "applied",
  "undoPreviewed",
  "undone",
  "failed",
]);

/** Which step a reply may arrive in; anything else is ignored. */
const AWAITING: Partial<Record<PlanActionEvent["type"], PlanActionStage>> = {
  prepared: "preparing",
  applied: "applying",
  undoPreviewed: "previewingUndo",
  undone: "undoing",
};

/** Whether any change in the report was saved with a way to undo it. */
export function hasUndoableChange(report: ApplyReport | null): boolean {
  return (
    report?.batch.outcomes.some(
      (outcome) => outcome.status === "succeeded" && outcome.historyEntryId,
    ) ?? false
  );
}

/** Where Undo returns to: the latest result, or the latest Undo result. */
function settled(state: PlanActionState): PlanActionStage {
  return state.undoReport ? "undone" : "result";
}

export function planAction(
  state: PlanActionState,
  event: PlanActionEvent,
): PlanActionState {
  // Late replies to an older request, or to a step the user has left, never
  // change what's on screen.
  if (REPLIES.has(event.type) && "request" in event) {
    if (event.request !== state.request) return state;
    const awaiting = AWAITING[event.type];
    if (awaiting && state.stage !== awaiting) return state;
  }

  switch (event.type) {
    case "prepareStarted":
      // A new preview replaces the plan; a running apply or Undo can't be left.
      if (state.stage === "applying" || state.stage === "undoing") return state;
      return {
        ...PLAN_ACTION_START,
        stage: "preparing",
        request: event.request,
      };
    case "prepared":
      return { ...state, stage: "preview", plan: event.plan };
    case "applyStarted":
      // Only the plan on screen can be approved, and only once it was shown
      // without a refusal next to it.
      return state.stage === "preview" && state.plan && !state.error
        ? { ...state, stage: "applying", request: event.request }
        : state;
    case "applied":
      return { ...state, stage: "result", report: event.report };
    case "undoPreviewStarted":
      return (state.stage === "result" || state.stage === "undone") &&
        hasUndoableChange(state.report)
        ? {
            ...state,
            stage: "previewingUndo",
            request: event.request,
            undoPreflight: null,
            error: null,
          }
        : state;
    case "undoPreviewed":
      return { ...state, stage: "undoPreview", undoPreflight: event.preflight };
    case "undoStarted":
      // Only a preflight with no conflict, confirmed as shown, is undone.
      return state.stage === "undoPreview" &&
        state.undoPreflight?.undoable &&
        !state.error
        ? { ...state, stage: "undoing", request: event.request }
        : state;
    case "undone":
      return {
        ...state,
        stage: "undone",
        undoPreflight: null,
        undoReport: event.report,
      };
    case "failed":
      // Thrown errors are refusals made before any write. Each keeps the step
      // the user can act on: the preview, or the Undo preflight.
      switch (state.stage) {
        case "preparing":
          return { ...state, stage: "idle", error: event.error };
        case "applying":
          return { ...state, stage: "preview", error: event.error };
        case "previewingUndo":
          return { ...state, stage: settled(state), error: event.error };
        case "undoing":
          return { ...state, stage: "undoPreview", error: event.error };
        default:
          return state;
      }
    case "closeUndo":
      return state.stage === "previewingUndo" || state.stage === "undoPreview"
        ? {
            ...state,
            stage: settled(state),
            request: event.request,
            undoPreflight: null,
            error: null,
          }
        : state;
    case "dismissError":
      return { ...state, error: null };
    case "reset":
      return { ...PLAN_ACTION_START, request: event.request };
  }
}

/** True while the native core is working on a request from this action. */
export function planActionBusy(stage: PlanActionStage): boolean {
  return (
    stage === "preparing" ||
    stage === "applying" ||
    stage === "previewingUndo" ||
    stage === "undoing"
  );
}
