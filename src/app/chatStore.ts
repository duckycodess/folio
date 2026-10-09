import type { FolioErrorPayload } from "../domain/contracts";
import { folioError } from "../domain/errors";
import { addTurn, MAX_TURNS, updateTurn, type AskTurn } from "./askAct";

/**
 * One conversation: Olio's floating chat and the Ask & Act page read and
 * write the same conversation, keyed by the open folder. Nothing here is
 * specific to either surface.
 */
export interface Conversation {
  id: string;
  folderId: string | undefined;
  scope: string;
  turns: AskTurn[];
  next: number;
  updatedAt: number;
}

export interface ChatState {
  conversations: Conversation[];
  activeId: string | null;
}

/** Oldest conversations are dropped first, across every folder. */
export const MAX_CONVERSATIONS = 20;

/**
 * What the stored history may take, in UTF-16 characters, well under the
 * browser's ~5 MB per-origin storage. Beyond it the oldest conversations are
 * left out of what is saved (they stay visible until the app closes).
 */
export const MAX_STORED_CHARS = 1_000_000;

export const EMPTY_STATE: ChatState = { conversations: [], activeId: null };

function newId(): string {
  return `c${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function emptyConversation(
  folderId: string | undefined,
  scope: string,
): Conversation {
  return {
    id: newId(),
    folderId,
    scope,
    turns: [],
    next: 0,
    updatedAt: Date.now(),
  };
}

/* ----------------------------------------------------- pure state helpers */

/** A folder's conversations, most recently active first. */
export function conversationsForFolder(
  state: ChatState,
  folderId: string | undefined,
): Conversation[] {
  return state.conversations
    .filter((conversation) => conversation.folderId === folderId)
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

export function findConversation(
  state: ChatState,
  id: string | null,
): Conversation | undefined {
  return id ? state.conversations.find((c) => c.id === id) : undefined;
}

/**
 * The conversation the chat should show for this folder: the active one if
 * it's already this folder's, else that folder's latest, else none yet.
 * Opening a different folder never surfaces another folder's conversation.
 */
export function activeConversation(
  state: ChatState,
  folderId: string | undefined,
): Conversation | undefined {
  const active = findConversation(state, state.activeId);
  if (active && active.folderId === folderId) return active;
  return conversationsForFolder(state, folderId)[0];
}

/** A short label for the history list, from the conversation's first turn. */
export function conversationTitle(conversation: Conversation): string {
  const first = conversation.turns[0]?.request.trim();
  if (!first) return "New conversation";
  return first.length > 60 ? `${first.slice(0, 60)}…` : first;
}

/** Oldest-first eviction once there are more than `max` conversations. */
export function pruned(state: ChatState, max: number): ChatState {
  if (state.conversations.length <= max) return state;
  const kept = [...state.conversations]
    .sort((a, b) => a.updatedAt - b.updatedAt)
    .slice(state.conversations.length - max);
  return { ...state, conversations: kept };
}

export function withNewConversation(
  state: ChatState,
  folderId: string | undefined,
  scope: string,
): { state: ChatState; id: string } {
  const conversation = emptyConversation(folderId, scope);
  return {
    state: pruned(
      {
        conversations: [...state.conversations, conversation],
        activeId: conversation.id,
      },
      MAX_CONVERSATIONS,
    ),
    id: conversation.id,
  };
}

export function withActiveId(state: ChatState, id: string): ChatState {
  return state.conversations.some((c) => c.id === id)
    ? { ...state, activeId: id }
    : state;
}

function withConversation(
  state: ChatState,
  id: string,
  change: (conversation: Conversation) => Conversation,
): ChatState {
  return {
    ...state,
    conversations: state.conversations.map((conversation) =>
      conversation.id === id
        ? { ...change(conversation), updatedAt: Date.now() }
        : conversation,
    ),
  };
}

export function withScope(
  state: ChatState,
  id: string,
  scope: string,
): ChatState {
  return withConversation(state, id, (c) => ({ ...c, scope }));
}

export function withTurnAdded(
  state: ChatState,
  id: string,
  turn: Omit<AskTurn, "id">,
): { state: ChatState; turnId: number } {
  const conversation = findConversation(state, id);
  const turnId = (conversation?.next ?? 0) + 1;
  return {
    state: withConversation(state, id, (c) => ({
      ...c,
      next: turnId,
      turns: addTurn(c.turns, { ...turn, id: turnId }),
    })),
    turnId,
  };
}

export function withTurnUpdated(
  state: ChatState,
  id: string,
  turnId: number,
  change: Partial<AskTurn>,
): ChatState {
  return withConversation(state, id, (c) => ({
    ...c,
    turns: updateTurn(c.turns, turnId, change),
  }));
}

export function withTurnsCleared(state: ChatState, id: string): ChatState {
  return withConversation(state, id, (c) => ({
    ...c,
    turns: c.turns.filter((turn) => turn.status === "running"),
  }));
}

/* --------------------------------------------------------- serialization */

type StoredTurn = Omit<AskTurn, "error"> & { error?: FolioErrorPayload };

/** Running turns can't be resumed after a reload, so they aren't kept. */
function persistableTurns(turns: AskTurn[]): StoredTurn[] {
  return turns
    .filter((turn) => turn.status !== "running")
    .map((turn) => ({ ...turn, error: turn.error?.toPayload() }));
}

/**
 * The stored form, at most `maxChars` long: the oldest conversations are
 * dropped until it fits, so a full storage quota can't silently stop saving.
 */
export function serializeChatState(
  state: ChatState,
  maxChars = MAX_STORED_CHARS,
): string {
  let conversations = [...state.conversations]
    .sort((a, b) => a.updatedAt - b.updatedAt)
    .map((conversation) => ({
      ...conversation,
      turns: persistableTurns(conversation.turns),
    }));
  for (;;) {
    const json = JSON.stringify({ activeId: state.activeId, conversations });
    if (json.length <= maxChars || conversations.length === 0) return json;
    conversations = conversations.slice(1);
  }
}

const FINISHED: ReadonlySet<unknown> = new Set(["done", "failed", "cancelled"]);

/**
 * Only well-formed, finished turns come back: stored data may be from an
 * older version, edited or damaged, and a bad turn must not break the chat.
 */
function reviveTurns(raw: unknown): AskTurn[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter(
      (turn): turn is Record<string, unknown> =>
        typeof turn === "object" &&
        turn !== null &&
        typeof turn.id === "number" &&
        typeof turn.request === "string" &&
        (turn.action === "find" || turn.action === "ask") &&
        FINISHED.has(turn.status) &&
        (turn.outcome === undefined ||
          (typeof turn.outcome === "object" &&
            turn.outcome !== null &&
            typeof (turn.outcome as { type?: unknown }).type === "string")),
    )
    .slice(-MAX_TURNS)
    .map((turn) => {
      const payload = turn.error as FolioErrorPayload | undefined;
      return {
        ...turn,
        error: payload
          ? folioError(payload.code, payload.message, payload.details)
          : undefined,
      } as AskTurn;
    });
}

function reviveConversations(raw: unknown): Conversation[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter(
      (item): item is Record<string, unknown> =>
        typeof item === "object" && item !== null,
    )
    .map((item) => ({
      id: typeof item.id === "string" ? item.id : newId(),
      folderId: typeof item.folderId === "string" ? item.folderId : undefined,
      scope: typeof item.scope === "string" ? item.scope : "",
      next: typeof item.next === "number" ? item.next : 0,
      updatedAt:
        typeof item.updatedAt === "number" ? item.updatedAt : Date.now(),
      turns: reviveTurns(item.turns),
    }));
}

/** The safe direction on bad or missing data: start with no history. */
export function deserializeChatState(raw: string | null): ChatState {
  if (!raw) return EMPTY_STATE;
  try {
    const parsed = JSON.parse(raw);
    return {
      conversations: reviveConversations(parsed?.conversations).slice(
        -MAX_CONVERSATIONS,
      ),
      activeId: typeof parsed?.activeId === "string" ? parsed.activeId : null,
    };
  } catch {
    return EMPTY_STATE;
  }
}

/* ------------------------------------------------------- the live store */

const STORAGE_KEY = "folio.chat.conversations";

// Storage can be missing, full or throw; the chat then starts empty, which
// is the safe direction (nothing is lost that was ever applied to a file).
function load(): ChatState {
  try {
    return deserializeChatState(window.localStorage.getItem(STORAGE_KEY));
  } catch {
    return EMPTY_STATE;
  }
}

function save(next: ChatState) {
  try {
    window.localStorage.setItem(STORAGE_KEY, serializeChatState(next));
  } catch {
    // Not remembered beyond this session.
  }
}

let state: ChatState = load();
const listeners = new Set<() => void>();

function update(change: (state: ChatState) => ChatState) {
  state = change(state);
  save(state);
  listeners.forEach((listener) => listener());
}

export function subscribeChat(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function chatSnapshot(): ChatState {
  return state;
}

/** Makes sure this folder has an active conversation, creating one if not. */
export function ensureActiveConversation(folderId: string | undefined): string {
  const existing = activeConversation(state, folderId);
  if (existing) {
    if (state.activeId !== existing.id)
      update((s) => withActiveId(s, existing.id));
    return existing.id;
  }
  const { state: next, id } = withNewConversation(state, folderId, "");
  update(() => next);
  return id;
}

/** Starts a fresh conversation for this folder and makes it active. */
export function newConversation(
  folderId: string | undefined,
  scope: string,
): string {
  const { state: next, id } = withNewConversation(state, folderId, scope);
  update(() => next);
  return id;
}

export function openConversation(id: string) {
  update((s) => withActiveId(s, id));
}

export function setConversationScope(id: string, scope: string) {
  update((s) => withScope(s, id, scope));
}

export function addTurnTo(id: string, turn: Omit<AskTurn, "id">): number {
  const { state: next, turnId } = withTurnAdded(state, id, turn);
  update(() => next);
  return turnId;
}

export function updateTurnIn(
  id: string,
  turnId: number,
  change: Partial<AskTurn>,
) {
  update((s) => withTurnUpdated(s, id, turnId, change));
}

export function clearTurnsIn(id: string) {
  update((s) => withTurnsCleared(s, id));
}

/** Kept on this device only; this clears every folder's history. */
export function deleteAllConversations() {
  update(() => EMPTY_STATE);
}
