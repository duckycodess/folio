import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { cancelGeneration } from "../adapters/ai";
import {
  addCollectionMembers,
  keepCollection,
  listCollections,
  removeCollection,
  removeCollectionMembers,
  renameCollection,
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
  remove: (collectionId: string) => void;
  removeMember: (collectionId: string, documentId: string) => void;
  /** Adds one file; the file itself stays where it is. */
  addMember: (collectionId: string, documentId: string) => Promise<boolean>;
  suggestions: SuggestState;
  /** Resolves once the groups arrive, fail or are stopped. */
  suggest: () => Promise<void>;
  /**
   * Drops the analysis and stops the model writing names. The runtime runs
   * one generation at a time, so this stops whichever request holds it.
   */
  stopSuggest: () => void;
  /** Forgets earlier suggestions without stopping any generation. */
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

  function replace(updated: VirtualCollection) {
    setCollections((list) =>
      list.map((item) => (item.id === updated.id ? updated : item)),
    );
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
      setCollections((list) => [collection, ...list]);
    } catch (cause) {
      dispatch({ type: "keepFailed", groupId, error: toFolioError(cause) });
    }
  }

  async function rename(collectionId: string, name: string) {
    if (!folderId) return false;
    try {
      replace(await renameCollection(folderId, collectionId, name));
      setError(null);
      return true;
    } catch (cause) {
      setError(toFolioError(cause));
      return false;
    }
  }

  async function remove(collectionId: string) {
    if (!folderId) return;
    try {
      await removeCollection(folderId, collectionId);
      setCollections((list) => list.filter((item) => item.id !== collectionId));
      setError(null);
    } catch (cause) {
      setError(toFolioError(cause));
    }
  }

  async function addMember(collectionId: string, documentId: string) {
    if (!folderId) return false;
    try {
      replace(await addCollectionMembers(folderId, collectionId, [documentId]));
      setError(null);
      return true;
    } catch (cause) {
      setError(toFolioError(cause));
      return false;
    }
  }

  async function removeMember(collectionId: string, documentId: string) {
    if (!folderId) return;
    try {
      replace(
        await removeCollectionMembers(folderId, collectionId, [documentId]),
      );
      setError(null);
    } catch (cause) {
      setError(toFolioError(cause));
    }
  }

  return {
    available: Boolean(folderId) && workspace.nativeAvailable,
    collections,
    error,
    dismissError: () => setError(null),
    reload,
    rename,
    remove: (collectionId) => void remove(collectionId),
    addMember,
    removeMember: (collectionId, documentId) =>
      void removeMember(collectionId, documentId),
    suggestions,
    suggest,
    stopSuggest: () => {
      if (suggestions.status !== "grouping") return;
      dispatch({ type: "stopped", request: ++next.current });
      void cancelGeneration().catch(() => undefined);
    },
    clearSuggestions: () =>
      dispatch({ type: "reset", request: ++next.current }),
    editName: (groupId, name) => dispatch({ type: "editName", groupId, name }),
    toggleMember: (groupId, documentId) =>
      dispatch({ type: "toggleMember", groupId, documentId }),
    keep: (groupId) => void keep(groupId),
  };
}
