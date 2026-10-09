import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import {
  answerQuestion,
  cancelGeneration,
  indexStatus,
  interpretRequest,
  isAvailable,
  rebuildIndex,
  semanticSearch,
} from "../adapters/ai";
import type {
  DocumentRecord,
  ProviderIndexStatus,
  SearchResult,
} from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import {
  addTurn,
  inScope,
  summaryTarget,
  updateTurn,
  type AskOutcome,
  type AskTurn,
} from "./askAct";
import { summarize } from "./useSummary";
import type { WorkspaceState } from "./useWorkspace";

const RESULT_LIMIT = 20;

export interface AskActController {
  /** The open folder's id; Ask & Act needs a folder in the desktop app. */
  folderId: string | undefined;
  desktop: boolean;
  /** "" is the whole open folder. */
  scope: string;
  setScope: (folder: string) => void;
  index: ProviderIndexStatus | null;
  preparing: boolean;
  indexError: FolioError | null;
  prepare: () => void;
  turns: AskTurn[];
  busy: boolean;
  find: (request: string) => void;
  ask: (request: string) => void;
  /** Summarizes the chosen file of an earlier turn. */
  chooseForSummary: (turnId: number, document: DocumentRecord) => void;
  cancel: () => void;
  clear: () => void;
}

/**
 * The Ask & Act session for the open folder: scope and replies. It outlives
 * the page, so leaving Ask & Act (or a reply finishing while away) loses
 * nothing until another folder is opened.
 */
interface AskSession {
  folderId: string | undefined;
  scope: string;
  turns: AskTurn[];
  next: number;
}

let current: AskSession = {
  folderId: undefined,
  scope: "",
  turns: [],
  next: 0,
};
const listeners = new Set<() => void>();

function update(change: (state: AskSession) => AskSession) {
  current = change(current);
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Opens Ask & Act on a scope chosen elsewhere (Home's folder filter). The
 * user can still change it there; nothing is sent.
 */
export function prefillAskScope(folderId: string | undefined, scope: string) {
  update((state) =>
    state.folderId === folderId
      ? { ...state, scope }
      : { folderId, scope, turns: [], next: 0 },
  );
}

/**
 * Ask & Act: one read-only request at a time over the open folder. Retrieved
 * text is shown as evidence only; nothing here can approve or apply a change.
 */
export function useAskAct(workspace: WorkspaceState): AskActController {
  const desktop = isAvailable();
  const folderId =
    desktop && workspace.source === "folder"
      ? workspace.workspace?.id
      : undefined;
  const session = useSyncExternalStore(subscribe, () => current);
  const { scope, turns } = session;
  const [index, setIndex] = useState<ProviderIndexStatus | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [indexError, setIndexError] = useState<FolioError | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  // A different folder starts afresh; the same folder keeps its replies.
  useEffect(() => {
    if (current.folderId !== folderId)
      update(() => ({ folderId, scope: "", turns: [], next: 0 }));
    setIndex(null);
    if (!folderId) return;
    indexStatus()
      .then((status) => mounted.current && setIndex(status))
      .catch(() => undefined);
  }, [folderId]);

  const busy = turns.some((turn) => turn.status === "running");

  // Written to the session even if Ask & Act was left meanwhile.
  function finish(id: number, change: Partial<AskTurn>) {
    update((state) => ({
      ...state,
      turns: updateTurn(state.turns, id, change),
    }));
  }

  async function run(
    action: AskTurn["action"],
    request: string,
    work: (folder: string) => Promise<AskOutcome>,
  ) {
    const text = request.trim();
    if (!folderId || busy || !text) return;
    const id = current.next + 1;
    update((state) => ({
      ...state,
      next: id,
      turns: addTurn(state.turns, {
        id,
        request: text,
        action,
        status: "running",
      }),
    }));
    try {
      finish(id, { status: "done", outcome: await work(folderId) });
    } catch (cause) {
      const error = toFolioError(cause);
      finish(
        id,
        error.code === "cancelled"
          ? { status: "cancelled" }
          : { status: "failed", error },
      );
    } finally {
      // The first request prepares the folder; show what it prepared.
      indexStatus()
        .then((status) => mounted.current && setIndex(status))
        .catch(() => undefined);
    }
  }

  async function search(
    folder: string,
    query: string,
  ): Promise<SearchResult[]> {
    return inScope(await semanticSearch(folder, query, RESULT_LIMIT), scope);
  }

  async function interpret(
    folder: string,
    request: string,
  ): Promise<AskOutcome> {
    const meaning = await interpretRequest(folder, request);
    switch (meaning.status) {
      case "nonMutating": {
        const query = meaning.targetQuery?.trim() || request;
        if (meaning.intent === "question")
          return {
            type: "answer",
            result: await answerQuestion(folder, request),
          };
        const results = await search(folder, query);
        if (meaning.intent === "search")
          return { type: "results", query, results };
        const target = summaryTarget(results);
        if (!target)
          return {
            type: "chooseFile",
            purpose: "summarize",
            candidates: results,
          };
        void summarize(folder, target.id);
        return { type: "summary", document: target };
      }
      case "needsFileSelection":
        return {
          type: "chooseFile",
          purpose: "change",
          candidates: inScope(meaning.candidates, scope),
        };
      case "needsClarification":
        return { type: "clarify", question: meaning.question };
      case "proposal":
        return { type: "proposal", proposal: meaning.proposal };
      case "unsupported":
        return { type: "unsupported", reason: meaning.reason };
      case "invalidModelOutput":
        return { type: "unreadable" };
    }
  }

  return {
    folderId,
    desktop,
    scope,
    setScope: (folder) => update((state) => ({ ...state, scope: folder })),
    index: index && index.workspaceId === folderId ? index : null,
    preparing,
    indexError,
    prepare: () => {
      if (!folderId || preparing) return;
      setPreparing(true);
      setIndexError(null);
      rebuildIndex(folderId)
        .then((status) => mounted.current && setIndex(status))
        .catch((cause) => mounted.current && setIndexError(toFolioError(cause)))
        .finally(() => mounted.current && setPreparing(false));
    },
    turns,
    busy,
    find: (request) =>
      void run("find", request, async (folder) => ({
        type: "results",
        query: request.trim(),
        results: await search(folder, request),
      })),
    ask: (request) =>
      void run("ask", request, (folder) => interpret(folder, request)),
    chooseForSummary: (turnId, document) => {
      if (!folderId) return;
      void summarize(folderId, document.id);
      finish(turnId, { outcome: { type: "summary", document } });
    },
    cancel: () => void cancelGeneration().catch(() => undefined),
    clear: () =>
      update((state) => ({
        ...state,
        turns: state.turns.filter((turn) => turn.status === "running"),
      })),
  };
}
