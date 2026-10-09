import { describe, expect, it } from "vitest";
import type {
  CollectionSuggestions,
  SuggestedCollection,
  VirtualCollection,
} from "../domain/contracts";
import { folioError } from "../domain/errors";
import {
  SUGGEST_START,
  suggestFlow,
  type SuggestEvent,
} from "./collectionSuggestions";

const GROUP = {
  id: "suggested-1",
  provenance: "embedding",
  spaceFingerprint: "space",
  cohesion: 0.9,
  members: [
    { documentId: "w:a.md", relativePath: "a.md", contentHash: "sha256:a" },
    { documentId: "w:b.md", relativePath: "b.md", contentHash: "sha256:b" },
  ],
  name: { text: "Deadlines", citations: [], modelId: "m", revision: "r" },
} as unknown as SuggestedCollection;

const RESULT: CollectionSuggestions = {
  status: "grouped",
  analyzedDocumentCount: 2,
  truncated: false,
  naming: "named",
  groups: [GROUP],
};

const KEPT = { id: "collection-1", name: "Deadlines" } as VirtualCollection;

function run(...events: SuggestEvent[]) {
  return events.reduce(suggestFlow, SUGGEST_START);
}

const ready: SuggestEvent[] = [
  { type: "started", request: 1 },
  { type: "received", request: 1, result: RESULT },
];

describe("suggested collections in Organize", () => {
  it("drafts each group from its generated name", () => {
    const state = run(...ready);
    expect(state.status).toBe("ready");
    expect(state.drafts[GROUP.id]).toEqual({
      name: "Deadlines",
      chosen: ["w:a.md", "w:b.md"],
    });
  });

  it("ignores a reply to an older request, or one after Stop", () => {
    expect(
      run(
        { type: "started", request: 1 },
        { type: "started", request: 2 },
        { type: "received", request: 1, result: RESULT },
      ).status,
    ).toBe("grouping");
    const stopped = run(
      { type: "started", request: 1 },
      { type: "stopped", request: 2 },
      { type: "received", request: 1, result: RESULT },
      { type: "failed", request: 1, error: folioError("internal", "late") },
    );
    // Stopped stays on screen, so focus has somewhere to go.
    expect(stopped.status).toBe("stopped");
    expect(stopped.result).toBeNull();
  });

  it("edits a draft until the group is kept, then freezes it", () => {
    const kept = run(
      ...ready,
      { type: "editName", groupId: GROUP.id, name: "Mga deadline" },
      { type: "toggleMember", groupId: GROUP.id, documentId: "w:b.md" },
      { type: "keepStarted", groupId: GROUP.id },
      { type: "kept", groupId: GROUP.id, collection: KEPT },
      { type: "editName", groupId: GROUP.id, name: "Changed later" },
    );
    expect(kept.kept[GROUP.id]).toBe(KEPT);
    expect(kept.drafts[GROUP.id]).toEqual({
      name: "Mga deadline",
      chosen: ["w:a.md"],
    });
    expect(kept.keeping).toBeNull();
    // A kept group can't be kept twice.
    expect(
      suggestFlow(kept, { type: "keepStarted", groupId: GROUP.id }).keeping,
    ).toBeNull();
  });

  it("keeps the draft when keeping is refused", () => {
    const error = folioError("targetChanged", "A file changed.");
    const refused = run(
      ...ready,
      { type: "keepStarted", groupId: GROUP.id },
      { type: "keepFailed", groupId: GROUP.id, error },
    );
    expect(refused.keepError).toEqual({ groupId: GROUP.id, error });
    expect(refused.kept).toEqual({});
    expect(refused.drafts[GROUP.id]?.name).toBe("Deadlines");
  });
});

describe("analyzing again", () => {
  it("keeps an edited name and unticked files for a group that comes back", () => {
    const refused = run(
      ...ready,
      { type: "editName", groupId: GROUP.id, name: "Mga deadline" },
      { type: "toggleMember", groupId: GROUP.id, documentId: "w:b.md" },
      { type: "keepStarted", groupId: GROUP.id },
      {
        type: "keepFailed",
        groupId: GROUP.id,
        error: folioError("targetChanged", "A file changed. Analyze again."),
      },
    );
    const grown = {
      ...GROUP,
      members: [
        ...GROUP.members,
        { documentId: "w:c.md", relativePath: "c.md", contentHash: "sha256:c" },
      ],
    } as unknown as SuggestedCollection;
    const again = [
      { type: "cleared", request: 2 },
      { type: "started", request: 3 },
      {
        type: "received",
        request: 3,
        result: { ...RESULT, groups: [grown] },
      },
    ] as SuggestEvent[];
    const state = again.reduce(suggestFlow, refused);
    // The new file is ticked; the one the user unticked stays unticked.
    expect(state.drafts[GROUP.id]).toEqual({
      name: "Mga deadline",
      chosen: ["w:a.md", "w:c.md"],
    });
    expect(state.keepError).toBeNull();
  });

  it("still shows a kept group as kept", () => {
    const state = run(
      ...ready,
      { type: "keepStarted", groupId: GROUP.id },
      { type: "kept", groupId: GROUP.id, collection: KEPT },
      { type: "started", request: 2 },
      { type: "received", request: 2, result: RESULT },
    );
    expect(state.kept[GROUP.id]).toBe(KEPT);
  });

  it("forgets everything when the folder changes", () => {
    const state = run(
      ...ready,
      { type: "editName", groupId: GROUP.id, name: "Mga deadline" },
      { type: "reset", request: 2 },
    );
    expect(state.drafts).toEqual({});
    expect(state.result).toBeNull();
  });
});
