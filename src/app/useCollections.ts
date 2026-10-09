import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import {
  keepCollection,
  listCollections,
  removeCollection,
  removeCollectionMembers,
  renameCollection,
  stopSuggestions,
  suggestCollections,
} from "../adapters/collections";
import { cleanCollectionName, keptMembers } from "../domain/collections";
import type { VirtualCollection } from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import {
  SUGGEST_START,
  suggestFlow,
  type SuggestState,
} from "./collectionSuggestions";
import type { WorkspaceState } from "./useWorkspace";

export interface CollectionsController {
  /** Collections need an open folder in the desktop app. */
  available: boolean;
  collections: VirtualCollection[];
  /** The last refused list, rename or removal; files are never involved. */
  error: FolioError | null;
  dismissError: () => void;
  /** Lists again, e.g. after Folio renamed or moved files. */
  reload: () => void;
  rename: (collectionId: string, name: string) => Promise<boolean>;
  /** Resolves true once the collection is gone; its files stay where they are. */
  remove: (collectionId: string) => Promise<boolean>;
  removeMember: (collectionId: string, documentId: string) => void;
  suggestions: SuggestState;
  suggest: () => void;
  /**
   * Stops grouping natively, before it takes the generation slot or by
   * cancelling the naming it holds; never another feature's generation.
   */
  stopSuggest: () => void;
  /** Hides the suggestions before a new analysis, keeping the drafts. */
  clearSuggestions: () => void;
  editName: (groupId: string, name: string) => void;
  toggleMember: (groupId: string, documentId: string) => void;
  keep: (groupId: string) => void;
}

export function useCollections(
  workspace: WorkspaceState,
): CollectionsController {
  const folderId =
    workspace.source === "folder" ? workspace.workspace?.id : undefined;
  const [collections, setCollections] = useState<VirtualCollection[]>([]);
  const [error, setError] = useState<FolioError | null>(null);
  const [suggestions, dispatch] = useReducer(suggestFlow, SUGGEST_START);
  const next = useRef(0);
  // The folder a reply belongs to; a reply for an earlier folder is dropped.
  const current = useRef(folderId);
  current.current = folderId;

  const reload = useCallback(() => {
    if (!folderId) return;
    listCollections(folderId)
      .then((list) => {
        if (current.current === folderId) setCollections(list);
      })
      .catch((cause) => {
        if (current.current === folderId) setError(toFolioError(cause));
      });
  }, [folderId]);

  useEffect(() => {
    setCollections([]);
    setError(null);
    dispatch({ type: "reset", request: ++next.current });
    reload();
  }, [folderId, reload]);

  /** Only replies for the folder still open may change what's shown. */
  const stillOpen = (folder: string) => current.current === folder;

  function replace(updated: VirtualCollection) {
    setCollections((list) =>
      list.map((item) => (item.id === updated.id ? updated : item)),
    );
  }

  function refused(folder: string, cause: unknown) {
    if (stillOpen(folder)) setError(toFolioError(cause));
  }

  async function suggest() {
    const request = ++next.current;
    dispatch({ type: "started", request });
    if (!folderId) return;
    try {
      const result = await suggestCollections(folderId);
      dispatch({ type: "received", request, result });
    } catch (cause) {
      dispatch({ type: "failed", request, error: toFolioError(cause) });
    }
  }

  async function keep(groupId: string) {
    const group = suggestions.result?.groups.find(
      (item) => item.id === groupId,
    );
    const draft = suggestions.drafts[groupId];
    if (!folderId || !group || !draft) return;
    dispatch({ type: "keepStarted", groupId });
    try {
      const collection = await keepCollection(
        folderId,
        cleanCollectionName(draft.name),
        keptMembers(group, draft),
      );
      dispatch({ type: "kept", groupId, collection });
      if (stillOpen(folderId)) setCollections((list) => [collection, ...list]);
    } catch (cause) {
      dispatch({ type: "keepFailed", groupId, error: toFolioError(cause) });
    }
  }

  async function rename(collectionId: string, name: string) {
    if (!folderId) return false;
    try {
      const renamed = await renameCollection(folderId, collectionId, name);
      if (!stillOpen(folderId)) return false;
      replace(renamed);
      setError(null);
      return true;
    } catch (cause) {
      refused(folderId, cause);
      return false;
    }
  }

  async function remove(collectionId: string) {
    if (!folderId) return false;
    try {
      await removeCollection(folderId, collectionId);
      if (!stillOpen(folderId)) return false;
      setCollections((list) => list.filter((item) => item.id !== collectionId));
      setError(null);
      return true;
    } catch (cause) {
      refused(folderId, cause);
      return false;
    }
  }

  async function removeMember(collectionId: string, documentId: string) {
    if (!folderId) return;
    try {
      const updated = await removeCollectionMembers(folderId, collectionId, [
        documentId,
      ]);
      if (!stillOpen(folderId)) return;
      replace(updated);
      setError(null);
    } catch (cause) {
      refused(folderId, cause);
    }
  }

  return {
    available: Boolean(folderId) && workspace.nativeAvailable,
    collections,
    error,
    dismissError: () => setError(null),
    reload,
    rename,
    remove,
    removeMember: (collectionId, documentId) =>
      void removeMember(collectionId, documentId),
    suggestions,
    suggest: () => void suggest(),
    stopSuggest: () => {
      if (suggestions.status !== "grouping") return;
      dispatch({ type: "stopped", request: ++next.current });
      void stopSuggestions().catch(() => undefined);
    },
    clearSuggestions: () => {
      if (suggestions.status === "grouping")
        void stopSuggestions().catch(() => undefined);
      dispatch({ type: "cleared", request: ++next.current });
    },
    editName: (groupId, name) => dispatch({ type: "editName", groupId, name }),
    toggleMember: (groupId, documentId) =>
      dispatch({ type: "toggleMember", groupId, documentId }),
    keep: (groupId) => void keep(groupId),
  };
}
