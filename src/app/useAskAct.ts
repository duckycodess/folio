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
import { mockReply } from "../adapters/mockChat";
import type {
  DocumentRecord,
  ProviderIndexStatus,
  SearchResult,
} from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import {
  inScope,
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
    indexStatus()
      .then((status) => mounted.current && setIndex(status))
      .catch(() => undefined);
  }, [folderId]);

  const busy = turns.some((turn) => turn.status === "running");

  async function run(
    action: AskTurn["action"],
    request: string,
    work: (
      folder: string,
      onProgress: (partial: AskOutcome) => void,
    ) => Promise<AskOutcome>,
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
    });
    const onProgress = (partial: AskOutcome) =>
      updateTurnIn(conversationId, id, { outcome: partial });
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
    setScope: (folder) =>
      setConversationScope(ensureActiveConversation(folderId), folder),
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
      void run("find", request, async (folder, onProgress) =>
        desktop
          ? {
              type: "results",
              query: request.trim(),
              results: await search(folder, request),
            }
          : mockReply(request, onProgress),
      ),
    ask: (request) =>
      void run("ask", request, (folder, onProgress) =>
        desktop ? interpret(folder, request) : mockReply(request, onProgress),
      ),
    chooseForSummary: (turnId, document) => {
      if (!folderId || !conversation) return;
      void summarize(folderId, document.id);
      updateTurnIn(conversation.id, turnId, {
        outcome: { type: "summary", document },
      });
    },
    cancel: () => void cancelGeneration().catch(() => undefined),
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
