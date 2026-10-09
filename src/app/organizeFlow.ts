import type {
  ActionPlan,
  ApplyReport,
  DocumentId,
  FileChangeSuggestions,
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

/**
 * Where a suggestion came from: the title-based name, a name the local model
 * wrote, or a move into a folder whose files are closer in meaning.
 */
export type SuggestionKind = "title" | "model" | "move";

export function suggestionKey(
  kind: SuggestionKind,
  documentId: DocumentId,
): string {
  return `${kind}:${documentId}`;
}

function documentOfKey(key: string): DocumentId {
  return key.slice(key.indexOf(":") + 1);
}

/**
 * The local models' renames and moves, which arrive after the analysis.
 * `stopping` waits for the reply to a stopped request, which carries the names
 * already written and the moves; `stopped` is a stopped request that failed.
 */
export interface AssistState {
  status: "idle" | "working" | "stopping" | "stopped" | "ready" | "failed";
  /** Only the reply to this request may change what's shown. */
  request: number;
  result: FileChangeSuggestions | null;
  error: FolioError | null;
}

export const ASSIST_IDLE: AssistState = {
  status: "idle",
  request: 0,
  result: null,
  error: null,
};

export interface OrganizeState {
  stage: OrganizeStage;
  /** Only the reply to this request may move the flow on. */
  request: number;
  progress: IndexProgress | null;
  suggestions: OrganizationSuggestions | null;
  /**
   * Suggestions the user picked, by `suggestionKey`. At most one per file,
   * since a plan can change each file only once.
   */
  chosen: string[];
  assist: AssistState;
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
  /**
   * The user pressed Stop. It takes a new request number, so a scan that had
   * already finished can't bring its suggestions back afterwards.
   */
  | { type: "stopAnalyze"; request: number }
  /** Choosing a suggestion drops any other one already chosen for that file. */
  | { type: "toggle"; key: string }
  | { type: "assistStarted"; request: number }
  | { type: "assisted"; request: number; result: FileChangeSuggestions }
  | { type: "assistFailed"; request: number; error: FolioError }
  /**
   * Stop keeps the request number: the stopped request's reply still lands,
   * with the names written before the stop and the moves already found.
   */
  | { type: "assistStopped" }
  | { type: "prepareStarted"; request: number; operations: FileOperation[] }
  | { type: "prepared"; request: number; plan: ActionPlan }
  | { type: "applyStarted"; request: number }
  | { type: "applied"; request: number; report: ApplyReport }
  | { type: "failed"; request: number; error: FolioError }
  | { type: "backToSuggestions" }
  | { type: "dismissError" }
  /** Starts over; a new request number drops replies still in flight. */
  | { type: "reset"; request: number };

export const ORGANIZE_START: OrganizeState = {
  stage: "idle",
  request: 0,
  progress: null,
  suggestions: null,
  chosen: [],
  assist: ASSIST_IDLE,
  operations: [],
  plan: null,
  report: null,
  error: null,
};

/** The operations of the chosen suggestions, in the order they are listed. */
export function chosenOperations(state: OrganizeState): FileOperation[] {
  const listed: [string, FileOperation][] = [
    ...(state.suggestions?.filenames ?? []).map(
      (item) =>
        [suggestionKey("title", item.documentId), item.operation] as [
          string,
          FileOperation,
        ],
    ),
    ...(state.assist.result?.filenames ?? []).map(
      (item) =>
        [suggestionKey("model", item.documentId), item.operation] as [
          string,
          FileOperation,
        ],
    ),
    ...(state.assist.result?.destinations ?? []).map(
      (item) =>
        [suggestionKey("move", item.documentId), item.operation] as [
          string,
          FileOperation,
        ],
    ),
  ];
  return listed
    .filter(([key]) => state.chosen.includes(key))
    .map(([, operation]) => operation);
}

const REPLIES = new Set<OrganizeEvent["type"]>([
  "progress",
  "analyzed",
  "analyzeCancelled",
  "prepared",
  "applied",
  "failed",
]);

function awaitingReply(state: OrganizeState): boolean {
  return (
    state.assist.status === "working" || state.assist.status === "stopping"
  );
}

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
        chosen: [],
        assist: ASSIST_IDLE,
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
    case "stopAnalyze":
      return state.stage === "analyzing"
        ? {
            ...state,
            stage: resting(state),
            request: event.request,
            progress: null,
          }
        : state;
    case "toggle": {
      if (state.chosen.includes(event.key))
        return {
          ...state,
          chosen: state.chosen.filter((key) => key !== event.key),
        };
      const document = documentOfKey(event.key);
      return {
        ...state,
        chosen: [
          ...state.chosen.filter((key) => documentOfKey(key) !== document),
          event.key,
        ],
      };
    }
    case "assistStarted":
      return {
        ...state,
        assist: { ...ASSIST_IDLE, status: "working", request: event.request },
      };
    case "assisted":
      if (event.request !== state.assist.request || !awaitingReply(state))
        return state;
      return {
        ...state,
        assist: { ...state.assist, status: "ready", result: event.result },
      };
    case "assistFailed":
      if (event.request !== state.assist.request || !awaitingReply(state))
        return state;
      // A stopped request's refusal is the stop itself, not a failure to show.
      return {
        ...state,
        assist:
          state.assist.status === "stopping"
            ? { ...state.assist, status: "stopped" }
            : { ...state.assist, status: "failed", error: event.error },
      };
    case "assistStopped":
      return state.assist.status === "working"
        ? { ...state, assist: { ...state.assist, status: "stopping" } }
        : state;
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
      return { ...ORGANIZE_START, request: event.request };
  }
}
