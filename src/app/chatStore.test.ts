import { describe, expect, it } from "vitest";
import { folioError } from "../domain/errors";
import {
  activeConversation,
  conversationsForFolder,
  conversationTitle,
  deserializeChatState,
  EMPTY_STATE,
  MAX_CONVERSATIONS,
  pruned,
  serializeChatState,
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

  it("starts empty for missing or corrupt data", () => {
    expect(deserializeChatState(null)).toEqual(EMPTY_STATE);
    expect(deserializeChatState("not json")).toEqual(EMPTY_STATE);
    expect(deserializeChatState('{"conversations": "nope"}')).toEqual(
      EMPTY_STATE,
    );
  });
});
