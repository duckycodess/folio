import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import {
  answerQuestion,
  cancelGeneration,
  indexStatus,
  interpretRequest,
  isAvailable,
  onPreparingProgress,
  rebuildIndex,
  semanticSearch,
} from "../adapters/ai";
import type {
  DocumentRecord,
  PreparingProgress,
  ProviderIndexStatus,
  SearchResult,
} from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import { mergeFolderResults } from "../domain/searchEvidence";
import {
  inScope,
  planAsk,
  namedFiles,
  summaryTarget,
  type AskOutcome,
  type AskTurn,
} from "./askAct";
import {
  activeConversation,
  addTurnTo,
  chatSnapshot,
  clearTurnsIn,
  conversationsForFolder,
  conversationTitle,
  deleteAllConversations as clearAllConversations,
  ensureActiveConversation,
  newConversation as startConversation,
  openConversation as activateConversation,
  setConversationScope,
  subscribeChat,
  updateTurnIn,
} from "./chatStore";
import { summarize } from "./useSummary";
import type { WorkspaceState } from "./useWorkspace";

const RESULT_LIMIT = 20;

/**
 * Follows what a request is doing while it prepares the folder. Returns the
 * function that stops listening, safe to call before the listener is ready.
 */
function watchPreparing(
  folderId: string,
  onUpdate: (progress: PreparingProgress) => void,
): () => void {
  let stopped = false;
  let unlisten: (() => void) | undefined;
  onPreparingProgress((update) => {
    if (!stopped && update.workspaceId === folderId) onUpdate(update);
  })
    .then((stop) => {
      if (stopped) stop();
      else unlisten = stop;
    })
    .catch(() => {
      // Progress is a nicety; the request's own result is what counts.
    });
  return () => {
    stopped = true;
    unlisten?.();
  };
}

/**
 * The browser preview's practice replies. `TAURI_ENV_PLATFORM` is set while
 * `tauri build` runs, so in the desktop build this branch is dead code and
 * the mock adapter is not bundled at all.
 */
async function practiceReply(
  request: string,
  onProgress: (partial: AskOutcome) => void,
): Promise<AskOutcome> {
  if (import.meta.env.TAURI_ENV_PLATFORM)
    throw new Error("Practice replies are not part of the desktop app.");
  const { mockReply } = await import("../adapters/mockChat");
  return mockReply(request, onProgress);
}

export interface ConversationSummary {
  id: string;
  title: string;
  updatedAt: number;
}

export interface AskActController {
  /** The open folder's id; Ask & Act needs a folder in the desktop app. */
  folderId: string | undefined;
  desktop: boolean;
  /** "" is the whole open folder. */
  scope: string;
  setScope: (folder: string) => void;
  index: ProviderIndexStatus | null;
  preparing: boolean;
  /** What the request in flight is preparing, when it is preparing. */
  progress: PreparingProgress | null;
  indexError: FolioError | null;
  prepare: () => void;
  turns: AskTurn[];
  busy: boolean;
  find: (request: string) => void;
  /** `chosen` is the file the user picked; a change must target it. */
  ask: (request: string, chosen?: DocumentRecord) => void;
  /**
   * Continues an earlier "which file?" turn with the file the user picked:
   * summarizes it, answers the question from it, or reads the change request
   * again for it.
   */
  chooseFile: (turnId: number, document: DocumentRecord) => void;
  cancel: () => void;
  /** Stops whatever holds the local model (a summary, Model Lab, …). */
  stopRunning: () => Promise<void>;
  clear: () => void;
  /** The conversation currently open. Both Ask & Act and the floating chat
   * read and write this same id: there is no copy to keep in sync. */
  conversationId: string | null;
  /** This folder's other conversations, most recent first. */
  history: ConversationSummary[];
  openConversation: (id: string) => void;
  newConversation: () => void;
  /** Kept on this device only; clears every folder's history. */
  deleteAllConversations: () => void;
}

/**
 * Ask & Act: one read-only request at a time over the open folder. Retrieved
 * text is shown as evidence only; nothing here can approve or apply a change.
 * Shared by the full Ask & Act page and the floating Olio chat (#66) through
 * one conversation store (`chatStore.ts`), keyed by the open folder.
 */
export function useAskAct(workspace: WorkspaceState): AskActController {
  const desktop = isAvailable();
  const folderId =
    desktop && workspace.source === "folder"
      ? workspace.workspace?.id
      : undefined;
  const snapshot = useSyncExternalStore(subscribeChat, chatSnapshot);
  const conversation = activeConversation(snapshot, folderId);
  const scope = conversation?.scope ?? "";
  const turns = conversation?.turns ?? [];
  const [index, setIndex] = useState<ProviderIndexStatus | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [progress, setProgress] = useState<PreparingProgress | null>(null);
  const [indexError, setIndexError] = useState<FolioError | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  // A different folder gets its own conversation (its latest, or a fresh
  // one); it never shows another folder's turns.
  useEffect(() => {
    ensureActiveConversation(folderId);
    setIndex(null);
    if (!folderId) return;
    indexStatus(folderId)
      .then((status) => mounted.current && setIndex(status))
      .catch(() => undefined);
  }, [folderId]);

  // One request at a time across every conversation and folder: starting a
  // new conversation, or switching to another, must not start a second one.
  const busy = snapshot.conversations.some((other) =>
    other.turns.some((turn) => turn.status === "running"),
  );

  async function run(
    action: AskTurn["action"],
    request: string,
    work: (
      folder: string,
      onProgress: (partial: AskOutcome) => void,
    ) => Promise<AskOutcome>,
    chosen?: DocumentRecord,
  ) {
    const text = request.trim();
    // Without a folder, the desktop app has nothing to search; the browser
    // preview's practice mode (below) needs no folder at all.
    if ((desktop && !folderId) || busy || !text) return;
    // Captured now, so the reply lands in this conversation even if the
    // user switches to another one (or another folder) meanwhile.
    const conversationId = ensureActiveConversation(folderId);
    const id = addTurnTo(conversationId, {
      request: text,
      action,
      status: "running",
      chosen,
    });
    const onProgress = (partial: AskOutcome) =>
      updateTurnIn(conversationId, id, { outcome: partial });
    const stopWatching =
      desktop && folderId
        ? watchPreparing(folderId, (update) => {
            if (mounted.current) setProgress(update);
          })
        : undefined;
    try {
      updateTurnIn(conversationId, id, {
        status: "done",
        outcome: await work(folderId ?? "", onProgress),
      });
    } catch (cause) {
      const error = toFolioError(cause);
      updateTurnIn(
        conversationId,
        id,
        error.code === "cancelled"
          ? { status: "cancelled" }
          : { status: "failed", error },
      );
    } finally {
      stopWatching?.();
      if (mounted.current) setProgress(null);
      // The first request prepares the folder; show what it prepared.
      indexStatus(folderId)
        .then((status) => mounted.current && setIndex(status))
        .catch(() => undefined);
    }
  }

  /**
   * The index's matches, with files the request names by file name first:
   * the index scores only file text, so it can't find a file by its name.
   */
  async function search(
    folder: string,
    query: string,
  ): Promise<{ results: SearchResult[]; namesOnly: boolean }> {
    const { named, partial } = namedFiles(workspace.documents, query);
    let indexed: SearchResult[];
    let namesOnly = false;
    try {
      indexed = await semanticSearch(folder, query, RESULT_LIMIT);
    } catch (cause) {
      // Only "no search model yet" falls back to names, and the turn says
      // so. Any other failure (I/O, a mismatched space) is reported, never
      // hidden behind name matches that look like a full search.
      const error = toFolioError(cause);
      if (
        error.code !== "modelNotInstalled" ||
        (!named.length && !partial.length)
      )
        throw cause;
      indexed = [];
      namesOnly = true;
    }
    return {
      results: inScope(
        mergeFolderResults(
          workspace.documents,
          [...named, ...indexed],
          partial,
        ),
        scope,
      ).slice(0, RESULT_LIMIT),
      namesOnly,
    };
  }

  async function interpret(
    folder: string,
    request: string,
    chosen?: DocumentRecord,
  ): Promise<AskOutcome> {
    const meaning = await interpretRequest(folder, request, chosen?.id);
    const step = planAsk(meaning, request, chosen, scope);
    switch (step.kind) {
      case "answer":
        return {
          type: "answer",
          result: await answerQuestion(folder, request, step.documentId),
        };
      case "results": {
        const { results, namesOnly } = await search(folder, step.query);
        return { type: "results", query: step.query, results, namesOnly };
      }
      case "summarize":
        void summarize(folder, step.document.id);
        return { type: "summary", document: step.document };
      case "findSummaryTarget": {
        const { results } = await search(folder, step.query);
        // A request that writes out one file's full name means that file
        // (#105), among the results the search found.
        const { exact } = namedFiles(workspace.documents, request);
        const target = summaryTarget(
          results,
          exact.filter((document) =>
            results.some((result) => result.document.id === document.id),
          ),
        );
        if (!target)
          return {
            type: "chooseFile",
            purpose: "summarize",
            candidates: results,
          };
        void summarize(folder, target.id);
        return { type: "summary", document: target };
      }
      case "outcome":
        return step.outcome;
    }
  }

  return {
    folderId,
    desktop,
    scope,
    setScope: (folder) =>
      setConversationScope(ensureActiveConversation(folderId), folder),
    index: index && index.workspaceId === folderId ? index : null,
    preparing,
    progress,
    indexError,
    prepare: () => {
      if (!folderId || preparing) return;
      setPreparing(true);
      setIndexError(null);
      const stopWatching = watchPreparing(folderId, (update) => {
        if (mounted.current) setProgress(update);
      });
      rebuildIndex(folderId)
        .then((status) => mounted.current && setIndex(status))
        .catch((cause) => mounted.current && setIndexError(toFolioError(cause)))
        .finally(() => {
          stopWatching();
          if (mounted.current) {
            setProgress(null);
            setPreparing(false);
          }
        });
    },
    turns,
    busy,
    find: (request) =>
      void run("find", request, async (folder, onProgress) =>
        desktop
          ? {
              type: "results",
              query: request.trim(),
              ...(await search(folder, request)),
            }
          : practiceReply(request, onProgress),
      ),
    ask: (request, chosen) =>
      void run(
        "ask",
        request,
        (folder, onProgress) =>
          desktop
            ? interpret(folder, request, chosen)
            : practiceReply(request, onProgress),
        chosen,
      ),
    chooseFile: (turnId, document) => {
      if (!folderId || !conversation) return;
      const turn = conversation.turns.find((other) => other.id === turnId);
      const outcome = turn?.outcome;
      if (!turn || outcome?.type !== "chooseFile") return;
      switch (outcome.purpose) {
        case "summarize":
          void summarize(folderId, document.id);
          updateTurnIn(conversation.id, turnId, {
            outcome: { type: "summary", document },
          });
          return;
        case "question":
          // The intent is already known; answer from the chosen file.
          void run(
            "ask",
            turn.request,
            async (folder) => ({
              type: "answer",
              result: await answerQuestion(folder, turn.request, document.id),
            }),
            document,
          );
          return;
        case "change":
          void run(
            "ask",
            turn.request,
            (folder) => interpret(folder, turn.request, document),
            document,
          );
          return;
      }
    },
    cancel: () => void cancelGeneration().catch(() => undefined),
    stopRunning: () => cancelGeneration().catch(() => undefined),
    clear: () => conversation && clearTurnsIn(conversation.id),
    conversationId: conversation?.id ?? null,
    history: conversationsForFolder(snapshot, folderId)
      .filter((c) => c.id !== conversation?.id)
      .map((c) => ({
        id: c.id,
        title: conversationTitle(c),
        updatedAt: c.updatedAt,
      })),
    openConversation: (id) => activateConversation(id),
    newConversation: () => void startConversation(folderId, ""),
    deleteAllConversations: () => clearAllConversations(),
  };
}
