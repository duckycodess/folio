import { useEffect, useMemo, useRef, useState } from "react";
import {
  chooseWorkspace,
  loadFixtureDocuments,
  nativeAvailable,
  readNativeDocument,
} from "../adapters/workspace";
import type {
  DocumentRecord,
  Relationship,
  SearchResult,
  WorkspaceInfo,
} from "../domain/contracts";
import { discoverExplicitReferences, keywordSearch } from "../domain/discovery";
import { toFolioError } from "../domain/errors";

/**
 * Whose files are listed. The desktop app starts with `none` until the user
 * adds a folder or asks for the samples; the browser preview only has samples.
 */
export type WorkspaceSourceKind = "none" | "samples" | "folder";

export interface WorkspaceState {
  documents: DocumentRecord[];
  /** `null` unless the user has chosen a folder. */
  workspace: WorkspaceInfo | null;
  source: WorkspaceSourceKind;
  nativeAvailable: boolean;
  /** True while the sample files are on their way. */
  loading: boolean;
  query: string;
  setQuery: (query: string) => void;
  results: SearchResult[];
  relationships: Relationship[];
  selected: DocumentRecord | undefined;
  /** Related documents for the selection, from explicit links only. */
  neighbors: DocumentRecord[];
  busy: boolean;
  error: string;
  notice: string;
  dismissNotice: () => void;
  selectDocument: (document: DocumentRecord) => Promise<void>;
  clearSelection: () => void;
  selectFolder: () => Promise<void>;
  /** Desktop only: list the bundled sample files before adding a folder. */
  showSamples: () => void;
}

export function useWorkspace(): WorkspaceState {
  const [documents, setDocuments] = useState<DocumentRecord[]>([]);
  const [workspace, setWorkspace] = useState<WorkspaceInfo | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [samplesRequested, setSamplesRequested] = useState(!nativeAvailable);
  const [loading, setLoading] = useState(!nativeAvailable);
  // Only the latest folder or file request may update state.
  const request = useRef(0);
  // Once a folder is open, late-arriving sample files must not replace it,
  // even when the folder is empty.
  const folderOpened = useRef(false);

  useEffect(() => {
    if (!samplesRequested) return;
    let active = true;
    loadFixtureDocuments()
      .then((fixtures) => {
        if (active && !folderOpened.current) setDocuments(fixtures);
      })
      .catch((cause) => {
        if (active) setError(toFolioError(cause).message);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [samplesRequested]);

  const selected = documents.find((document) => document.id === selectedId);
  const results = useMemo(
    () => keywordSearch(documents, query),
    [documents, query],
  );
  const relationships = useMemo(
    () => discoverExplicitReferences(documents),
    [documents],
  );
  const neighbors = useMemo(() => {
    const ids = new Set(
      relationships
        .filter(
          (edge) =>
            edge.sourceId === selectedId || edge.targetId === selectedId,
        )
        .map((edge) =>
          edge.sourceId === selectedId ? edge.targetId : edge.sourceId,
        ),
    );
    return documents.filter((document) => ids.has(document.id));
  }, [relationships, selectedId, documents]);

  async function selectDocument(document: DocumentRecord) {
    const current = ++request.current;
    setSelectedId(document.id);
    setError("");
    if (!workspace || document.content !== undefined) {
      setBusy(false);
      return;
    }
    setBusy(true);
    try {
      const read = await readNativeDocument(workspace.id, document);
      if (current !== request.current) return;
      setDocuments((all) =>
        all.map((item) => (item.id === read.id ? read : item)),
      );
    } catch (cause) {
      if (current === request.current) setError(toFolioError(cause).message);
    } finally {
      if (current === request.current) setBusy(false);
    }
  }

  async function selectFolder() {
    const current = ++request.current;
    setError("");
    setBusy(true);
    try {
      const chosen = await chooseWorkspace();
      if (current !== request.current || !chosen) return;
      folderOpened.current = true;
      setLoading(false);
      setWorkspace(chosen.info);
      setDocuments(chosen.documents);
      setSelectedId("");
      setQuery("");
      setNotice(
        chosen.skipped.length
          ? `${chosen.skipped.length} file(s) couldn't be identified and aren't listed.`
          : "",
      );
    } catch (cause) {
      if (current === request.current) setError(toFolioError(cause).message);
    } finally {
      if (current === request.current) setBusy(false);
    }
  }

  return {
    documents,
    workspace,
    source: workspace ? "folder" : samplesRequested ? "samples" : "none",
    nativeAvailable,
    loading,
    query,
    setQuery,
    results,
    relationships,
    selected,
    neighbors,
    busy,
    error,
    notice,
    dismissNotice: () => setNotice(""),
    selectDocument,
    clearSelection: () => setSelectedId(""),
    selectFolder,
    showSamples: () => {
      if (samplesRequested || folderOpened.current) return;
      setLoading(true);
      setSamplesRequested(true);
    },
  };
}
