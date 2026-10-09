import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import { failSoon, simulatedFailure } from "../adapters/simulate";
import {
  chooseWorkspace,
  listFolder,
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
import { toFolioError, type FolioError } from "../domain/errors";
import { actionReducer, IDLE, type ActionState } from "./actionState";

/** A failure on screen, with the step that can be retried. */
export interface Failure {
  error: FolioError;
  retry?: () => void;
}

/** What opening a folder produced, shown only after the native core says so. */
export interface FolderOpened {
  name: string;
  files: number;
}

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
  /** The desktop app, or the browser preview simulating a folder failure. */
  canChooseFolder: boolean;
  /** True while the sample files are on their way. */
  loading: boolean;
  query: string;
  setQuery: (query: string) => void;
  results: SearchResult[];
  relationships: Relationship[];
  selected: DocumentRecord | undefined;
  busy: boolean;
  failure: Failure | null;
  dismissFailure: () => void;
  notice: string;
  dismissNotice: () => void;
  folderAction: ActionState<FolderOpened>;
  dismissFolderResult: () => void;
  selectDocument: (document: DocumentRecord) => Promise<void>;
  clearSelection: () => void;
  selectFolder: () => Promise<void>;
  /**
   * Lists the open folder again after Folio changed it. Paths, and so
   * document identities, may have changed; a vanished selection is cleared.
   */
  refreshFolder: () => Promise<void>;
  /** Desktop only: list the bundled sample files before adding a folder. */
  showSamples: () => void;
}

export function useWorkspace(): WorkspaceState {
  const [documents, setDocuments] = useState<DocumentRecord[]>([]);
  const [workspace, setWorkspace] = useState<WorkspaceInfo | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [query, setQuery] = useState("");
  const [failure, setFailure] = useState<Failure | null>(null);
  const [folderAction, dispatchFolder] = useReducer(
    actionReducer<FolderOpened>,
    IDLE,
  );
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
        if (active) setFailure({ error: toFolioError(cause) });
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

  async function selectDocument(document: DocumentRecord) {
    const current = ++request.current;
    setSelectedId(document.id);
    setFailure(null);
    const simulated = simulatedFailure("read");
    if (!simulated && (!workspace || document.content !== undefined)) {
      setBusy(false);
      return;
    }
    setBusy(true);
    try {
      const read = simulated
        ? await failSoon(simulated)
        : await readNativeDocument(workspace!.id, document);
      if (current !== request.current) return;
      setDocuments((all) =>
        all.map((item) => (item.id === read.id ? read : item)),
      );
    } catch (cause) {
      if (current === request.current)
        setFailure({
          error: toFolioError(cause),
          retry: () => void selectDocument(document),
        });
    } finally {
      if (current === request.current) setBusy(false);
    }
  }

  async function selectFolder() {
    const current = ++request.current;
    setFailure(null);
    setBusy(true);
    dispatchFolder({ type: "start", request: current });
    try {
      const simulated = simulatedFailure("folder");
      const chosen = simulated
        ? await failSoon(simulated)
        : await chooseWorkspace();
      if (current !== request.current) return;
      if (!chosen) {
        // The user closed the folder picker; nothing to report.
        dispatchFolder({ type: "reset" });
        return;
      }
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
      dispatchFolder({
        type: "nativeSucceeded",
        request: current,
        result: {
          name: folderName(chosen.info.rootPath),
          files: chosen.documents.length,
        },
      });
    } catch (cause) {
      if (current !== request.current) return;
      const error = toFolioError(cause);
      dispatchFolder({ type: "nativeFailed", request: current, error });
      setFailure({ error, retry: () => void selectFolder() });
    } finally {
      if (current === request.current) setBusy(false);
    }
  }

  async function refreshFolder() {
    if (!workspace) return;
    try {
      const listing = await listFolder(workspace.id);
      setDocuments(listing.documents);
      setSelectedId((id) =>
        listing.documents.some((document) => document.id === id) ? id : "",
      );
    } catch (cause) {
      setFailure({
        error: toFolioError(cause),
        retry: () => void refreshFolder(),
      });
    }
  }

  return {
    documents,
    workspace,
    source: workspace ? "folder" : samplesRequested ? "samples" : "none",
    nativeAvailable,
    canChooseFolder:
      nativeAvailable || simulatedFailure("folder") !== undefined,
    loading,
    query,
    setQuery,
    results,
    relationships,
    selected,
    busy,
    failure,
    dismissFailure: () => setFailure(null),
    notice,
    dismissNotice: () => setNotice(""),
    folderAction,
    dismissFolderResult: () => dispatchFolder({ type: "reset" }),
    selectDocument,
    clearSelection: () => setSelectedId(""),
    selectFolder,
    refreshFolder,
    showSamples: () => {
      if (samplesRequested || folderOpened.current) return;
      setLoading(true);
      setSamplesRequested(true);
    },
  };
}

/** The last segment of a folder path, on Windows or macOS. */
function folderName(rootPath: string): string {
  return rootPath.split(/[\\/]/).filter(Boolean).at(-1) ?? rootPath;
}
