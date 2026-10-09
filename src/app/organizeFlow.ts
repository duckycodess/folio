import type {
  ActionPlan,
  ApplyReport,
  DocumentId,
  FileOperation,
  IndexProgress,
  OrganizationSuggestions,
} from "../domain/contracts";
import type { FolioError } from "../domain/errors";

/**
 * Journey B as explicit steps: analyze → suggestions → exact preview →
 * approve and apply → result. The plan shown in `preview` is the one the
 * native core issued; the UI only echoes it back for approval.
 */
export type OrganizeStage =
  | "idle"
  | "analyzing"
  | "suggestions"
  | "preparing"
  | "preview"
  | "applying"
  | "result";

export interface OrganizeState {
  stage: OrganizeStage;
  /** Only the reply to this request may move the flow on. */
  request: number;
  progress: IndexProgress | null;
  suggestions: OrganizationSuggestions | null;
  /** Filename suggestions the user picked, by document. */
  chosen: DocumentId[];
  /** The operations last sent for preview, so "Preview again" can resend them. */
  operations: FileOperation[];
  plan: ActionPlan | null;
  report: ApplyReport | null;
  /** A refusal; the step it came from stays on screen. */
  error: FolioError | null;
}

export type OrganizeEvent =
  | { type: "analyzeStarted"; request: number }
  | { type: "progress"; request: number; progress: IndexProgress }
  | {
      type: "analyzed";
      request: number;
      suggestions: OrganizationSuggestions;
    }
  | { type: "analyzeCancelled"; request: number }
  | { type: "toggle"; documentId: DocumentId }
  | { type: "prepareStarted"; request: number; operations: FileOperation[] }
  | { type: "prepared"; request: number; plan: ActionPlan }
  | { type: "applyStarted"; request: number }
  | { type: "applied"; request: number; report: ApplyReport }
  | { type: "failed"; request: number; error: FolioError }
  | { type: "backToSuggestions" }
  | { type: "dismissError" }
  | { type: "reset" };

export const ORGANIZE_START: OrganizeState = {
  stage: "idle",
  request: 0,
  progress: null,
  suggestions: null,
  chosen: [],
  operations: [],
  plan: null,
  report: null,
  error: null,
};

const REPLIES = new Set<OrganizeEvent["type"]>([
  "progress",
  "analyzed",
  "analyzeCancelled",
  "prepared",
  "applied",
  "failed",
]);

/** Where the flow rests when there is no plan in flight. */
function resting(state: OrganizeState): OrganizeStage {
  return state.suggestions ? "suggestions" : "idle";
}

export function organizeFlow(
  state: OrganizeState,
  event: OrganizeEvent,
): OrganizeState {
  // Late replies to an older request never change what's on screen.
  if (REPLIES.has(event.type) && "request" in event)
    if (event.request !== state.request) return state;

  switch (event.type) {
    case "analyzeStarted":
      return {
        ...state,
        stage: "analyzing",
        request: event.request,
        progress: null,
        error: null,
      };
    case "progress":
      return state.stage === "analyzing"
        ? { ...state, progress: event.progress }
        : state;
    case "analyzed":
      return {
        ...state,
        stage: "suggestions",
        progress: null,
        suggestions: event.suggestions,
        chosen: [],
      };
    case "analyzeCancelled":
      return { ...state, stage: resting(state), progress: null };
    case "toggle":
      return {
        ...state,
        chosen: state.chosen.includes(event.documentId)
          ? state.chosen.filter((id) => id !== event.documentId)
          : [...state.chosen, event.documentId],
      };
    case "prepareStarted":
      return {
        ...state,
        stage: "preparing",
        request: event.request,
        operations: event.operations,
        plan: null,
        error: null,
      };
    case "prepared":
      return { ...state, stage: "preview", plan: event.plan };
    case "applyStarted":
      return state.plan
        ? { ...state, stage: "applying", request: event.request, error: null }
        : state;
    case "applied":
      return { ...state, stage: "result", report: event.report };
    case "failed":
      // Thrown errors are refusals made before any write. An apply refusal
      // keeps the preview, so the user can read it and preview again.
      return {
        ...state,
        stage:
          state.stage === "applying"
            ? "preview"
            : state.stage === "analyzing"
              ? resting(state)
              : state.stage === "preparing"
                ? resting(state)
                : state.stage,
        progress: null,
        error: event.error,
      };
    case "backToSuggestions":
      return { ...state, stage: resting(state), plan: null, error: null };
    case "dismissError":
      return { ...state, error: null };
    case "reset":
      return { ...ORGANIZE_START, request: state.request };
  }
}
