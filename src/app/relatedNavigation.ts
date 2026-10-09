import type {
  DocumentId,
  DocumentRecord,
  SourcePassage,
} from "../domain/contracts";

/** How the reader got to the current file through Related or evidence. */
export interface RelatedNavigation {
  /** Files opened from, most recent last; the way back to the origin. */
  trail: DocumentRecord[];
  /** The passage the reader should highlight. */
  focus: SourcePassage | null;
  /** The origin reopened by Back, which shows its Related tab again. */
  returnedTo: DocumentId | null;
  /** The selection Folio itself is about to make. */
  expected: DocumentId | null;
}

export type RelatedNavigationAction =
  | { type: "selectionChanged"; id: DocumentId | undefined }
  | { type: "openRelated"; from: DocumentRecord; to: DocumentRecord }
  | {
      type: "openPassage";
      passage: SourcePassage;
      current: DocumentRecord | undefined;
    }
  | { type: "back" };

export const NO_NAVIGATION: RelatedNavigation = {
  trail: [],
  focus: null,
  returnedTo: null,
  expected: null,
};

export function relatedNavigation(
  state: RelatedNavigation,
  action: RelatedNavigationAction,
): RelatedNavigation {
  switch (action.type) {
    case "selectionChanged":
      // A file picked anywhere else (the list, search) starts afresh: no
      // trail, and no highlight from an earlier visit.
      return action.id !== undefined && action.id === state.expected
        ? { ...state, expected: null }
        : NO_NAVIGATION;
    case "openRelated":
      return {
        trail: [...state.trail, action.from],
        focus: null,
        returnedTo: null,
        expected: action.to.id,
      };
    case "openPassage": {
      const { current, passage } = action;
      // Opening evidence from Graph may happen with no file open yet.
      const moves = current?.id !== passage.documentId;
      return {
        trail: moves && current ? [...state.trail, current] : state.trail,
        focus: passage,
        returnedTo: null,
        expected: moves ? passage.documentId : state.expected,
      };
    }
    case "back": {
      const origin = state.trail[state.trail.length - 1];
      if (!origin) return state;
      return {
        trail: state.trail.slice(0, -1),
        focus: null,
        returnedTo: origin.id,
        expected: origin.id,
      };
    }
  }
}
