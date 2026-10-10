import { invoke } from "@tauri-apps/api/core";
import type {
  CollectionSuggestions,
  DocumentId,
  KeptMember,
  VirtualCollection,
  WorkspaceId,
} from "../domain/contracts";
import { toFolioError } from "../domain/errors";

/**
 * Virtual collections (#78, ADR 0017). The native core stores them; none of
 * these commands changes a file, so none of them needs a plan or approval.
 */
async function call<T>(command: string, args?: Record<string, unknown>) {
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    throw toFolioError(cause);
  }
}

/**
 * Groups the folder's files by meaning with the local embedding model, and
 * names each group with the local generation model when one is set up. Stop
 * with `stopSuggestions`; a stopped request rejects with `cancelled`.
 */
export function suggestCollections(
  workspaceId: WorkspaceId,
): Promise<CollectionSuggestions> {
  return call<CollectionSuggestions>("suggest_collections", { workspaceId });
}

/**
 * Stops the running suggestions natively: before they take the generation
 * slot, or by cancelling the naming they hold. Another feature's generation is
 * never cancelled.
 */
export function stopSuggestions(): Promise<void> {
  return call<void>("stop_suggestions");
}

export function listCollections(
  workspaceId: WorkspaceId,
): Promise<VirtualCollection[]> {
  return call<VirtualCollection[]>("list_collections", { workspaceId });
}

/** Refused with `targetChanged` when a member changed since the analysis. */
export function keepCollection(
  workspaceId: WorkspaceId,
  name: string,
  members: KeptMember[],
): Promise<VirtualCollection> {
  return call<VirtualCollection>("keep_collection", {
    workspaceId,
    name,
    members,
  });
}

export function renameCollection(
  workspaceId: WorkspaceId,
  collectionId: string,
  name: string,
): Promise<VirtualCollection> {
  return call<VirtualCollection>("rename_collection", {
    workspaceId,
    collectionId,
    name,
  });
}

/** Removes the collection; its files stay where they are. */
export function removeCollection(
  workspaceId: WorkspaceId,
  collectionId: string,
): Promise<void> {
  return call<void>("remove_collection", { workspaceId, collectionId });
}

export function addCollectionMembers(
  workspaceId: WorkspaceId,
  collectionId: string,
  documentIds: DocumentId[],
): Promise<VirtualCollection> {
  return call<VirtualCollection>("add_collection_members", {
    workspaceId,
    collectionId,
    documentIds,
  });
}

/** Takes files out of the collection; the files themselves are not touched. */
export function removeCollectionMembers(
  workspaceId: WorkspaceId,
  collectionId: string,
  documentIds: DocumentId[],
): Promise<VirtualCollection> {
  return call<VirtualCollection>("remove_collection_members", {
    workspaceId,
    collectionId,
    documentIds,
  });
}
