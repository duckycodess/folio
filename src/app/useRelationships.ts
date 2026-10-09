import { useEffect, useMemo, useRef, useState } from "react";
import {
  listDuplicates,
  listIndexedDocuments,
  listRelationships,
} from "../adapters/workspace";
import {
  connectionsFor,
  localDuplicates,
  mergeRelationships,
  type Connection,
} from "../domain/connections";
import type {
  DocumentId,
  DocumentRecord,
  DuplicateGroup,
  Relationship,
  SourcePassage,
} from "../domain/contracts";
import { toFolioError } from "../domain/errors";
import type { WorkspaceState } from "./useWorkspace";

/**
 * Where the connections come from, so the UI can say what it might be missing:
 * links in files that were opened, or the persistent index of the whole folder.
 */
export type RelationshipCoverage =
  "samples" | "loading" | "indexed" | "notIndexed" | "failed" | "none";

export interface RelationshipsState {
  coverage: RelationshipCoverage;
  /** Plain-language reason when the index couldn't be read. */
  error: string;
  /** Index and opened-file relationships, merged without repeats. */
  relationships: Relationship[];
  duplicates: DuplicateGroup[];
  connectionsOf: (documentId: DocumentId) => Connection[];
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

export function useRelationships(
  workspace: WorkspaceState,
): RelationshipsState {
  const folderId = workspace.workspace?.id;
  const [indexed, setIndexed] = useState<{
    folderId: string;
    coverage: "indexed" | "notIndexed" | "failed";
    relationships: Relationship[];
    duplicates: DuplicateGroup[];
    error: string;
  } | null>(null);
  const [focus, setFocus] = useState<SourcePassage | null>(null);
  const [trail, setTrail] = useState<DocumentRecord[]>([]);
  const [returnedTo, setReturnedTo] = useState<DocumentId | null>(null);
  // Set while Folio itself changes the selection, so a selection made
  // elsewhere (the file list, search) can start a fresh trail.
  const expected = useRef<DocumentId | null>(null);

  useEffect(() => {
    if (!folderId) return;
    let active = true;
    Promise.all([
      listIndexedDocuments(folderId),
      listRelationships(folderId),
      listDuplicates(folderId),
    ])
      .then(([documents, relationships, duplicates]) => {
        if (!active) return;
        setIndexed({
          folderId,
          coverage: documents.length ? "indexed" : "notIndexed",
          relationships,
          duplicates,
          error: "",
        });
      })
      .catch((cause) => {
        if (!active) return;
        setIndexed({
          folderId,
          coverage: "failed",
          relationships: [],
          duplicates: [],
          error: toFolioError(cause).message,
        });
      });
    return () => {
      active = false;
    };
  }, [folderId]);

  const current = indexed?.folderId === folderId ? indexed : null;
  const relationships = useMemo(
    () =>
      mergeRelationships(current?.relationships ?? [], workspace.relationships),
    [current, workspace.relationships],
  );
  // The index re-reads bytes for the whole folder; hashes of files Folio has
  // already read (the samples, opened files) cover the rest.
  const duplicates = useMemo(
    () => [
      ...(current?.duplicates ?? []),
      ...localDuplicates(workspace.documents),
    ],
    [current, workspace.documents],
  );

  const selectedId = workspace.selected?.id;
  useEffect(() => {
    if (selectedId === expected.current) {
      expected.current = null;
      return;
    }
    expected.current = null;
    setTrail([]);
    setReturnedTo(null);
  }, [selectedId]);

  function select(document: DocumentRecord) {
    if (document.id !== selectedId) expected.current = document.id;
    void workspace.selectDocument(document);
  }

  const coverage: RelationshipCoverage =
    workspace.source === "samples"
      ? "samples"
      : !folderId
        ? "none"
        : (current?.coverage ?? "loading");

  return {
    coverage,
    error: current?.error ?? "",
    relationships,
    duplicates,
    connectionsOf: (documentId) =>
      connectionsFor(documentId, relationships, duplicates),
    focus,
    openPassage: (passage) => {
      const document = workspace.documents.find(
        (item) => item.id === passage.documentId,
      );
      if (!document) return;
      const origin = workspace.selected;
      if (origin && origin.id !== document.id)
        setTrail((items) => [...items, origin]);
      setFocus(passage);
      setReturnedTo(null);
      select(document);
    },
    trail,
    openRelated: (from, to) => {
      setTrail((items) => [...items, from]);
      setFocus(null);
      setReturnedTo(null);
      select(to);
    },
    back: () => {
      const origin = trail[trail.length - 1];
      if (!origin) return;
      setTrail((items) => items.slice(0, -1));
      setFocus(null);
      setReturnedTo(origin.id);
      select(origin);
    },
    returnedTo,
  };
}
