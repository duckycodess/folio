import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelIndexing,
  listIndexedDocuments,
  onIndexProgress,
  scanWorkspace,
  searchIndex,
} from "../adapters/workspace";
import type {
  IndexProgress,
  SearchResult,
  WorkspaceId,
} from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";

/**
 * Whether the open folder's text can be searched:
 * - `none`: no folder is open (samples, or nothing yet);
 * - `checking`: asking the index what it holds;
 * - `missing`: the folder isn't indexed, so only names are searched;
 * - `indexing`: a scan is running;
 * - `ready`: text search uses the index;
 * - `failed`: the index couldn't be read or built.
 */
export type IndexState =
  "none" | "checking" | "missing" | "indexing" | "ready" | "failed";

export interface FolderSearch {
  index: IndexState;
  progress: IndexProgress | null;
  indexFailure: FolioError | null;
  buildIndex: () => void;
  cancelIndex: () => void;
  /** Text matches from the index for the current query. */
  results: SearchResult[];
  searching: boolean;
  searchFailure: FolioError | null;
  retrySearch: () => void;
}

const DEBOUNCE_MS = 200;

export function useFolderSearch(
  folderId: WorkspaceId | undefined,
  query: string,
): FolderSearch {
  const [index, setIndex] = useState<IndexState>("none");
  const [progress, setProgress] = useState<IndexProgress | null>(null);
  const [indexFailure, setIndexFailure] = useState<FolioError | null>(null);
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchFailure, setSearchFailure] = useState<FolioError | null>(null);
  const [attempt, setAttempt] = useState(0);
  // Only the latest search may update the results.
  const latest = useRef(0);

  // What does the index hold for this folder?
  useEffect(() => {
    setResults([]);
    setIndexFailure(null);
    if (!folderId) {
      setIndex("none");
      return;
    }
    let active = true;
    setIndex("checking");
    listIndexedDocuments(folderId)
      .then((indexed) => {
        if (active) setIndex(indexed.length ? "ready" : "missing");
      })
      .catch((cause) => {
        if (!active) return;
        setIndex("failed");
        setIndexFailure(toFolioError(cause));
      });
    return () => {
      active = false;
    };
  }, [folderId]);

  // A scan started elsewhere (Organize's Analyze, for example) also makes the
  // folder's text searchable.
  useEffect(() => {
    if (!folderId) return;
    let stop: (() => void) | undefined;
    let active = true;
    onIndexProgress((update) => {
      if (update.workspaceId === folderId && update.phase === "done")
        setIndex((state) => (state === "indexing" ? state : "ready"));
    })
      .then((unlisten) => {
        if (active) stop = unlisten;
        else unlisten();
      })
      .catch(() => {});
    return () => {
      active = false;
      stop?.();
    };
  }, [folderId]);

  const terms = query.trim();
  useEffect(() => {
    const current = ++latest.current;
    setSearchFailure(null);
    if (!folderId || index !== "ready" || !terms) {
      setResults([]);
      setSearching(false);
      return;
    }
    setSearching(true);
    const timer = setTimeout(() => {
      searchIndex(folderId, terms, 50)
        .then((found) => {
          if (current === latest.current) setResults(found);
        })
        .catch((cause) => {
          if (current !== latest.current) return;
          setResults([]);
          setSearchFailure(toFolioError(cause));
        })
        .finally(() => {
          if (current === latest.current) setSearching(false);
        });
    }, DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [folderId, index, terms, attempt]);

  const buildIndex = useCallback(() => {
    if (!folderId) return;
    setIndex("indexing");
    setIndexFailure(null);
    setProgress(null);
    let unlisten: (() => void) | undefined;
    onIndexProgress((update) => {
      if (update.workspaceId === folderId) setProgress(update);
    })
      .then((stop) => {
        unlisten = stop;
      })
      .catch(() => {
        // Progress is a nicety; the scan's own result is what counts.
      });
    scanWorkspace(folderId)
      .then(() => setIndex("ready"))
      .catch((cause) => {
        const error = toFolioError(cause);
        // Cancelling leaves whatever was already indexed searchable.
        if (error.code === "cancelled") {
          listIndexedDocuments(folderId)
            .then((indexed) => setIndex(indexed.length ? "ready" : "missing"))
            .catch(() => setIndex("missing"));
          return;
        }
        setIndex("failed");
        setIndexFailure(error);
      })
      .finally(() => {
        unlisten?.();
        setProgress(null);
      });
  }, [folderId]);

  return {
    index,
    progress,
    indexFailure,
    buildIndex,
    cancelIndex: () => void cancelIndexing().catch(() => {}),
    results,
    searching,
    searchFailure,
    retrySearch: () => setAttempt((value) => value + 1),
  };
}
