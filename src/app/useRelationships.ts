import { useCallback, useEffect, useMemo, useReducer, useState } from "react";
import {
  listDuplicates,
  listIndexedDocuments,
  listRelationshipsWithDiagnostics,
} from "../adapters/workspace";
import {
  connectionsFor,
  localDuplicates,
  mergeRelationships,
  type Connection,
  type DuplicateSet,
} from "../domain/connections";
import type {
  DocumentId,
  DocumentRecord,
  Relationship,
  SourcePassage,
} from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import { NO_NAVIGATION, relatedNavigation } from "./relatedNavigation";
import type { WorkspaceState } from "./useWorkspace";

/**
 * Where the connections come from, so the UI can say what it might be missing:
 * links in files that were opened, or the persistent index of the whole folder.
 */
export type RelationshipCoverage =
  "samples" | "loading" | "indexed" | "notIndexed" | "failed" | "none";

export interface RelationshipsState {
  coverage: RelationshipCoverage;
  /** Why the index couldn't be read, for the shared recovery notice. */
  failure: FolioError | null;
  /** Index and opened-file relationships, merged without repeats. */
  relationships: Relationship[];
  /** Malformed native rows dropped at the UI boundary. */
  invalidRelationshipCount: number;
  duplicates: DuplicateSet[];
  connectionsOf: (documentId: DocumentId) => Connection[];
  /**
   * Views that show connections call this; the folder index (which re-reads
   * candidate duplicates) is only read once something needs it.
   */
  request: () => void;
  /** Re-read the index, for example after a scan or an applied change. */
  refresh: () => void;
  /** The passage the reader should show, set by opening evidence. */
  focus: SourcePassage | null;
  openPassage: (passage: SourcePassage) => void;
  /** Files opened from Related, most recent last; the way back to the origin. */
  trail: DocumentRecord[];
  openRelated: (from: DocumentRecord, to: DocumentRecord) => void;
  back: () => void;
  /** The origin reopened by `back`, which should show its Related tab again. */
  returnedTo: DocumentId | null;
}

interface IndexSnapshot {
  key: string;
  coverage: "indexed" | "notIndexed" | "failed";
  relationships: Relationship[];
  invalidRelationshipCount: number;
  duplicates: DuplicateSet[];
  failure: FolioError | null;
}

export function useRelationships(
  workspace: WorkspaceState,
): RelationshipsState {
  const folderId = workspace.workspace?.id;
  const [wanted, setWanted] = useState(false);
  const [generation, setGeneration] = useState(0);
  const [snapshot, setSnapshot] = useState<IndexSnapshot | null>(null);
  const [navigation, dispatch] = useReducer(relatedNavigation, NO_NAVIGATION);
  const key = `${folderId}#${generation}`;

  useEffect(() => {
    if (!folderId || !wanted) return;
    let active = true;
    Promise.all([
      listIndexedDocuments(folderId),
      listRelationshipsWithDiagnostics(folderId),
      listDuplicates(folderId),
    ])
      .then(([documents, relationshipListing, duplicates]) => {
        if (!active) return;
        setSnapshot({
          key,
          coverage: documents.length ? "indexed" : "notIndexed",
          relationships: relationshipListing.relationships,
          invalidRelationshipCount: relationshipListing.invalidCount,
          duplicates,
          failure: null,
        });
      })
      .catch((cause) => {
        if (!active) return;
        setSnapshot({
          key,
          coverage: "failed",
          relationships: [],
          invalidRelationshipCount: 0,
          duplicates: [],
          failure: toFolioError(cause),
        });
      });
    return () => {
      active = false;
    };
  }, [folderId, wanted, key]);

  const current = snapshot?.key === key ? snapshot : null;
  const relationships = useMemo(
    () =>
      mergeRelationships(current?.relationships ?? [], workspace.relationships),
    [current, workspace.relationships],
  );
  // An indexed folder's duplicate groups were confirmed by re-reading bytes.
  // Otherwise the hashes of files Folio has read (samples, opened files) are
  // all there is.
  const duplicates = useMemo(
    () =>
      current?.coverage === "indexed"
        ? current.duplicates
        : localDuplicates(workspace.documents),
    [current, workspace.documents],
  );

  const selectedId = workspace.selected?.id;
  useEffect(() => {
    dispatch({ type: "selectionChanged", id: selectedId });
  }, [selectedId]);

  const connectionsOf = useCallback(
    (documentId: DocumentId) =>
      connectionsFor(documentId, relationships, duplicates),
    [relationships, duplicates],
  );

  const request = useCallback(() => setWanted(true), []);
  const refresh = useCallback(() => setGeneration((value) => value + 1), []);

  const coverage: RelationshipCoverage =
    workspace.source === "samples"
      ? "samples"
      : !folderId
        ? "none"
        : (current?.coverage ?? "loading");

  function find(id: DocumentId) {
    return workspace.documents.find((item) => item.id === id);
  }

  return {
    coverage,
    failure: current?.failure ?? null,
    relationships,
    invalidRelationshipCount: current?.invalidRelationshipCount ?? 0,
    duplicates,
    connectionsOf,
    request,
    refresh,
    focus: navigation.focus,
    openPassage: (passage) => {
      const document = find(passage.documentId);
      if (!document) return;
      dispatch({ type: "openPassage", passage, current: workspace.selected });
      void workspace.selectDocument(document);
    },
    trail: navigation.trail,
    openRelated: (from, to) => {
      dispatch({ type: "openRelated", from, to });
      void workspace.selectDocument(to);
    },
    back: () => {
      const origin = navigation.trail[navigation.trail.length - 1];
      if (!origin) return;
      dispatch({ type: "back" });
      void workspace.selectDocument(origin);
    },
    returnedTo: navigation.returnedTo,
  };
}
