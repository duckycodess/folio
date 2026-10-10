import { afterEach, describe, expect, it, vi } from "vitest";
import { folioError } from "../domain/errors";
import { MAX_TURNS } from "./askAct";
import {
  activeConversation,
  conversationsForFolder,
  conversationTitle,
  deserializeChatState,
  EMPTY_STATE,
  load,
  MAX_CONVERSATIONS,
  MAX_STORED_CHARS,
  pruned,
  serializeChatState,
  STORAGE_KEY,
  withActiveId,
  withNewConversation,
  withScope,
  withTurnAdded,
  withTurnsCleared,
  withTurnUpdated,
  type ChatState,
} from "./chatStore";

describe("conversation isolation per folder", () => {
  it("never surfaces another folder's conversation", () => {
    const { state: withA } = withNewConversation(EMPTY_STATE, "a", "");
    const { state: withBoth } = withNewConversation(withA, "b", "");
    expect(conversationsForFolder(withBoth, "a")).toHaveLength(1);
    expect(conversationsForFolder(withBoth, "b")).toHaveLength(1);
    expect(activeConversation(withBoth, "a")?.folderId).toBe("a");
    // The active conversation belongs to "b"; folder "a" falls back to its own.
    expect(activeConversation(withBoth, "b")?.folderId).toBe("b");
  });

  it("has no conversation for a folder that never started one", () => {
    const { state } = withNewConversation(EMPTY_STATE, "a", "");
    expect(activeConversation(state, "c")).toBeUndefined();
  });
});

describe("conversation and turn caps", () => {
  it("drops the oldest conversations beyond the cap", () => {
    let state: ChatState = EMPTY_STATE;
    const ids: string[] = [];
    for (let i = 0; i < MAX_CONVERSATIONS + 5; i++) {
      const result = withNewConversation(state, "a", "");
      state = pruned(result.state, MAX_CONVERSATIONS);
      ids.push(result.id);
    }
    expect(state.conversations).toHaveLength(MAX_CONVERSATIONS);
    expect(state.conversations.some((c) => c.id === ids[0])).toBe(false);
    expect(state.conversations.some((c) => c.id === ids.at(-1))).toBe(true);
  });
});

describe("turns within one conversation", () => {
  function started() {
    const { state, id } = withNewConversation(EMPTY_STATE, "a", "");
    return { state, id };
  }

  it("adds, updates and keeps running turns on clear", () => {
    const { state: base, id } = started();
    const { state: withTurn, turnId } = withTurnAdded(base, id, {
      request: "find my notes",
      action: "find",
      status: "running",
    });
    expect(activeConversation(withTurn, "a")?.turns).toHaveLength(1);

    const done = withTurnUpdated(withTurn, id, turnId, {
      status: "done",
      outcome: { type: "results", query: "notes", results: [] },
    });
    expect(activeConversation(done, "a")?.turns[0].status).toBe("done");

    const { state: withSecond, turnId: secondId } = withTurnAdded(done, id, {
      request: "still running",
      action: "ask",
      status: "running",
    });
    const cleared = withTurnsCleared(withSecond, id);
    const remaining = activeConversation(cleared, "a")!.turns;
    expect(remaining).toHaveLength(1);
    expect(remaining[0].id).toBe(secondId);
  });

  it("changes scope without touching other conversations", () => {
    const { state: base, id } = started();
    const { state: withOther } = withNewConversation(base, "a", "");
    const scoped = withScope(withOther, id, "Projects");
    expect(scoped.conversations.find((c) => c.id === id)?.scope).toBe(
      "Projects",
    );
  });
});

describe("conversationTitle", () => {
  it("uses the first turn's request, trimmed and capped", () => {
    const { state, id } = withNewConversation(EMPTY_STATE, "a", "");
    const long = "x".repeat(80);
    const { state: withTurn } = withTurnAdded(state, id, {
      request: `  ${long}  `,
      action: "ask",
      status: "running",
    });
    const conversation = activeConversation(withTurn, "a")!;
    expect(conversationTitle(conversation)).toBe(`${long.slice(0, 60)}…`);
  });

  it("falls back for a conversation with no turns yet", () => {
    const { state, id } = withNewConversation(EMPTY_STATE, "a", "");
    expect(conversationTitle(activeConversation(state, "a")!)).toBe(
      "New conversation",
    );
    expect(id).toBeTruthy();
  });
});

describe("activating a conversation by id", () => {
  it("ignores an id that doesn't exist", () => {
    const { state } = withNewConversation(EMPTY_STATE, "a", "");
    const unchanged = withActiveId(state, "missing");
    expect(unchanged).toBe(state);
  });
});

describe("persistence round-trip", () => {
  it("keeps conversations, scope and finished turns across a reload", () => {
    const { state: base, id } = withNewConversation(
      EMPTY_STATE,
      "a",
      "Projects",
    );
    const { state: withTurn, turnId } = withTurnAdded(base, id, {
      request: "find my notes",
      action: "find",
      status: "running",
    });
    const finished = withTurnUpdated(withTurn, id, turnId, {
      status: "failed",
      error: folioError("modelNotInstalled", "No local model yet."),
    });
    const revived = deserializeChatState(serializeChatState(finished));
    const conversation = activeConversation(revived, "a")!;
    expect(conversation.scope).toBe("Projects");
    expect(conversation.turns[0].status).toBe("failed");
    expect(conversation.turns[0].error?.code).toBe("modelNotInstalled");
    expect(conversation.turns[0].error?.message).toBe("No local model yet.");
  });

  it("drops turns that were still running at reload", () => {
    const { state: base, id } = withNewConversation(EMPTY_STATE, "a", "");
    const { state: withTurn } = withTurnAdded(base, id, {
      request: "still thinking",
      action: "ask",
      status: "running",
    });
    const revived = deserializeChatState(serializeChatState(withTurn));
    expect(activeConversation(revived, "a")?.turns).toHaveLength(0);
  });

  it("drops malformed or unfinished stored turns and keeps the turn cap", () => {
    const good = (id: number) => ({
      id,
      request: `turn ${id}`,
      action: "find",
      status: "done",
    });
    const stored = JSON.stringify({
      activeId: "c1",
      conversations: [
        {
          id: "c1",
          folderId: "a",
          turns: [
            {},
            { ...good(1), request: undefined },
            { ...good(2), status: "running" },
            { ...good(3), outcome: "not an outcome" },
            ...Array.from({ length: 30 }, (_, index) => good(10 + index)),
          ],
        },
      ],
    });
    const conversation = activeConversation(deserializeChatState(stored), "a")!;
    expect(conversation.turns).toHaveLength(MAX_TURNS);
    expect(conversation.turns.every((turn) => turn.status === "done")).toBe(
      true,
    );
    expect(conversation.turns.at(-1)?.request).toBe("turn 39");
    expect(conversationTitle(conversation)).toBe("turn 20");
  });

  it("keeps the stored history under the size cap, oldest out first", () => {
    let state: ChatState = EMPTY_STATE;
    for (let index = 0; index < 3; index += 1) {
      const { state: next, id } = withNewConversation(state, "a", "");
      state = withTurnAdded(next, id, {
        request: "x".repeat(400),
        action: "find",
        status: "done",
      }).state;
      state = {
        ...state,
        conversations: state.conversations.map((conversation) =>
          conversation.id === id
            ? { ...conversation, updatedAt: index }
            : conversation,
        ),
      };
    }
    // Room for two of the three conversations, not all of them.
    const all = serializeChatState(state).length;
    const limit = all - 100;
    const json = serializeChatState(state, limit);
    expect(json.length).toBeLessThanOrEqual(limit);
    const kept = deserializeChatState(json).conversations;
    expect(kept.map((conversation) => conversation.updatedAt)).toEqual([1, 2]);
    expect(serializeChatState(state).length).toBeLessThan(MAX_STORED_CHARS);
  });

  it("starts empty for missing or corrupt data", () => {
    expect(deserializeChatState(null)).toEqual(EMPTY_STATE);
    expect(deserializeChatState("not json")).toEqual(EMPTY_STATE);
    expect(deserializeChatState('{"conversations": "nope"}')).toEqual(
      EMPTY_STATE,
    );
  });
});

function memoryStorage(entries: Record<string, string> = {}) {
  const items = new Map(Object.entries(entries));
  return {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => void items.set(key, value),
    removeItem: (key: string) => void items.delete(key),
    has: (key: string) => items.has(key),
  };
}

describe("history across launches", () => {
  afterEach(() => vi.unstubAllGlobals());

  const stored = serializeChatState(
    withNewConversation(EMPTY_STATE, "workspace-1", "").state,
  );

  it("starts a new launch empty and deletes history an older version kept", () => {
    const local = memoryStorage({ [STORAGE_KEY]: stored });
    vi.stubGlobal("window", {
      localStorage: local,
      sessionStorage: memoryStorage(),
    });
    expect(load()).toEqual(EMPTY_STATE);
    expect(local.has(STORAGE_KEY)).toBe(false);
  });

  it("keeps this launch's history when the window reloads", () => {
    vi.stubGlobal("window", {
      localStorage: memoryStorage(),
      sessionStorage: memoryStorage({ [STORAGE_KEY]: stored }),
    });
    expect(load().conversations).toHaveLength(1);
  });
});
