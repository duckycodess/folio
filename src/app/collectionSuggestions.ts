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
  /** `stopped` keeps the section on screen, so focus has somewhere to go. */
  status: "idle" | "grouping" | "ready" | "failed" | "stopped";
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
  /** Hides the suggestions before a new analysis, keeping the drafts. */
  | { type: "cleared"; request: number }
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
      // The drafts, what was kept and the last result stay: a group that
      // comes back has the same id, so the user's name, unticked files and
      // Keep survive analyzing again. The last result is hidden meanwhile.
      return {
        ...state,
        status: "grouping",
        request: event.request,
        error: null,
        keeping: null,
        keepError: null,
      };
    case "received": {
      if (event.request !== state.request || state.status !== "grouping")
        return state;
      const before = new Map(
        (state.result?.groups ?? []).map((group) => [
          group.id,
          group.members.map((member) => member.documentId),
        ]),
      );
      return {
        ...state,
        status: "ready",
        result: event.result,
        drafts: Object.fromEntries(
          event.result.groups.map((group) => {
            const earlier = state.drafts[group.id];
            if (!earlier) return [group.id, draftFor(group)];
            const unticked = (before.get(group.id) ?? []).filter(
              (id) => !earlier.chosen.includes(id),
            );
            return [
              group.id,
              {
                name: earlier.name,
                chosen: group.members
                  .map((member) => member.documentId)
                  .filter((id) => !unticked.includes(id)),
              },
            ];
          }),
        ),
      };
    }
    case "failed":
      if (event.request !== state.request || state.status !== "grouping")
        return state;
      return { ...state, status: "failed", error: event.error };
    case "stopped":
      return state.status === "grouping"
        ? { ...state, status: "stopped", request: event.request }
        : state;
    case "cleared":
      return {
        ...state,
        status: "idle",
        request: event.request,
        error: null,
        keepError: null,
      };
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
