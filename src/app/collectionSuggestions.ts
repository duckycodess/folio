import { draftFor, type CollectionDraft } from "../domain/collections";
import type {
  CollectionSuggestions,
  VirtualCollection,
} from "../domain/contracts";
import type { FolioError } from "../domain/errors";

/**
 * Organize's suggested collections, beside the duplicate and filename
 * suggestions. Keeping a group stores references only, so there is no plan
 * here: just the drafts the user edits and what was kept.
 */
export interface SuggestState {
  status: "idle" | "grouping" | "ready" | "failed";
  /** Only the reply to this request may change what's shown. */
  request: number;
  result: CollectionSuggestions | null;
  error: FolioError | null;
  drafts: Record<string, CollectionDraft>;
  /** The group being kept right now. */
  keeping: string | null;
  keepError: { groupId: string; error: FolioError } | null;
  /** Groups already kept, with the collection each became. */
  kept: Record<string, VirtualCollection>;
}

export type SuggestEvent =
  | { type: "started"; request: number }
  | { type: "received"; request: number; result: CollectionSuggestions }
  | { type: "failed"; request: number; error: FolioError }
  /** Stop takes a new request number, so a late reply can't come back. */
  | { type: "stopped"; request: number }
  | { type: "editName"; groupId: string; name: string }
  | { type: "toggleMember"; groupId: string; documentId: string }
  | { type: "keepStarted"; groupId: string }
  | { type: "kept"; groupId: string; collection: VirtualCollection }
  | { type: "keepFailed"; groupId: string; error: FolioError }
  | { type: "reset"; request: number };

export const SUGGEST_START: SuggestState = {
  status: "idle",
  request: 0,
  result: null,
  error: null,
  drafts: {},
  keeping: null,
  keepError: null,
  kept: {},
};

export function suggestFlow(
  state: SuggestState,
  event: SuggestEvent,
): SuggestState {
  switch (event.type) {
    case "started":
      return { ...SUGGEST_START, status: "grouping", request: event.request };
    case "received":
      if (event.request !== state.request || state.status !== "grouping")
        return state;
      return {
        ...state,
        status: "ready",
        result: event.result,
        drafts: Object.fromEntries(
          event.result.groups.map((group) => [group.id, draftFor(group)]),
        ),
      };
    case "failed":
      if (event.request !== state.request || state.status !== "grouping")
        return state;
      return { ...state, status: "failed", error: event.error };
    case "stopped":
      return state.status === "grouping"
        ? { ...SUGGEST_START, request: event.request }
        : state;
    case "editName": {
      const draft = state.drafts[event.groupId];
      if (!draft || state.kept[event.groupId]) return state;
      return {
        ...state,
        drafts: {
          ...state.drafts,
          [event.groupId]: { ...draft, name: event.name },
        },
      };
    }
    case "toggleMember": {
      const draft = state.drafts[event.groupId];
      if (!draft || state.kept[event.groupId]) return state;
      const chosen = draft.chosen.includes(event.documentId)
        ? draft.chosen.filter((id) => id !== event.documentId)
        : [...draft.chosen, event.documentId];
      return {
        ...state,
        drafts: { ...state.drafts, [event.groupId]: { ...draft, chosen } },
      };
    }
    case "keepStarted":
      if (state.keeping || state.kept[event.groupId]) return state;
      return { ...state, keeping: event.groupId, keepError: null };
    case "kept":
      if (state.keeping !== event.groupId) return state;
      return {
        ...state,
        keeping: null,
        kept: { ...state.kept, [event.groupId]: event.collection },
      };
    case "keepFailed":
      if (state.keeping !== event.groupId) return state;
      return {
        ...state,
        keeping: null,
        keepError: { groupId: event.groupId, error: event.error },
      };
    case "reset":
      return { ...SUGGEST_START, request: event.request };
  }
}
