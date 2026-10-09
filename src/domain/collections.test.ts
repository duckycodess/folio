import { describe, expect, it } from "vitest";
import {
  cleanCollectionName,
  draftFor,
  keepProblem,
  keptMembers,
  membersLabel,
  nameOrigin,
  nameProblem,
  suggestionsNotice,
} from "./collections";
import type {
  CollectionSuggestions,
  SourcePassage,
  SuggestedCollection,
  VirtualCollection,
} from "./contracts";

function passage(documentId: string, text: string): SourcePassage {
  return {
    documentId,
    documentContentHash: `sha256:${documentId}`,
    offsetUnit: "utf8Byte",
    start: 0,
    end: new TextEncoder().encode(text).length,
    text,
  };
}

const GROUP: SuggestedCollection = {
  id: "suggested-1",
  provenance: "embedding",
  spaceFingerprint: "folio-space-v1/e5/r1/q/384/p",
  cohesion: 0.93,
  members: ["thesis/outline.md", "thesis/balangkas.md", "notes/thesis.md"].map(
    (path) => ({
      documentId: `w:${path}`,
      relativePath: path,
      title: path,
      contentHash: `sha256:w:${path}`,
      passage: passage(`w:${path}`, "Thesis chapter plan"),
      similarity: 0.95,
    }),
  ),
  name: {
    text: "Thesis plans",
    citations: [passage("w:thesis/outline.md", "Thesis chapter plan")],
    modelId: "qwen",
    revision: "r1",
  },
};

const UNNAMED: SuggestedCollection = { ...GROUP, name: undefined };

describe("suggested collections", () => {
  it("starts from the model's name with every member chosen", () => {
    expect(draftFor(GROUP)).toEqual({
      name: "Thesis plans",
      chosen: GROUP.members.map((member) => member.documentId),
    });
    expect(draftFor(UNNAMED).name).toBe("");
  });

  it("labels a name as generated only while it is exactly the model's", () => {
    const draft = draftFor(GROUP);
    expect(nameOrigin(GROUP, draft)).toBe("generated");
    expect(nameOrigin(GROUP, { ...draft, name: "  Thesis   plans " })).toBe(
      "generated",
    );
    expect(nameOrigin(GROUP, { ...draft, name: "Tesis" })).toBe("edited");
    expect(nameOrigin(UNNAMED, { ...draft, name: "Tesis" })).toBe("typed");
    expect(nameOrigin(UNNAMED, draftFor(UNNAMED))).toBe("empty");
  });

  it("keeps only the chosen members, each pinned to the revision analyzed", () => {
    const draft = {
      ...draftFor(GROUP),
      chosen: ["w:thesis/outline.md", "w:notes/thesis.md"],
    };
    expect(keptMembers(GROUP, draft)).toEqual([
      {
        documentId: "w:thesis/outline.md",
        expectedContentHash: "sha256:w:thesis/outline.md",
      },
      {
        documentId: "w:notes/thesis.md",
        expectedContentHash: "sha256:w:notes/thesis.md",
      },
    ]);
  });

  it("needs a name and at least two files", () => {
    expect(keepProblem(GROUP, draftFor(GROUP))).toBeNull();
    expect(keepProblem(UNNAMED, draftFor(UNNAMED))).toBe(
      "Give the collection a name.",
    );
    expect(
      keepProblem(GROUP, { ...draftFor(GROUP), chosen: ["w:notes/thesis.md"] }),
    ).toBe("Keep at least two files in the collection.");
  });

  it("checks names as the native core does", () => {
    expect(cleanCollectionName("  Mga   tala\tsa proyekto ")).toBe(
      "Mga tala sa proyekto",
    );
    expect(nameProblem("   ")).toBe("Give the collection a name.");
    expect(nameProblem("é".repeat(80))).toBeNull();
    expect(nameProblem("é".repeat(81))).toBe("Use at most 80 characters.");
    expect(nameProblem("bad\u0007name")).toBe(
      "Remove the control characters from the name.",
    );
    expect(nameProblem("Thesis \u202efdp.exe")).toBe(
      "Remove the invisible formatting characters from the name.",
    );
  });
});

describe("how the analysis went", () => {
  const base: CollectionSuggestions = {
    status: "grouped",
    analyzedDocumentCount: 12,
    truncated: false,
    naming: "named",
    groups: [GROUP],
  };

  it("says nothing when every group was named", () => {
    expect(suggestionsNotice(base)).toBeNull();
    expect(suggestionsNotice({ ...base, naming: "notNeeded" })).toBeNull();
  });

  it("explains a missing model without claiming AI output", () => {
    expect(
      suggestionsNotice({
        ...base,
        status: "embeddingModelMissing",
        groups: [],
      })?.text,
    ).toMatch(/needs a local embedding model/);
    expect(
      suggestionsNotice({ ...base, naming: "generationModelMissing" })?.text,
    ).toMatch(/have no name yet/);
  });

  it("reports a failure, a stop and files left out", () => {
    const failed = suggestionsNotice({
      ...base,
      naming: "failed",
      namingError: {
        code: "providerBusy",
        message: "Another request is active.",
      },
      truncated: true,
    });
    expect(failed?.tone).toBe("warning");
    expect(failed?.text).toContain("first 12 text files");
    expect(failed?.text).toContain("Another request is active.");
    expect(suggestionsNotice({ ...base, naming: "cancelled" })?.text).toMatch(
      /Naming stopped/,
    );
  });
});

describe("kept collections", () => {
  it("counts missing members apart", () => {
    const collection: VirtualCollection = {
      id: "collection-1",
      workspaceId: "w",
      name: "Deadlines",
      createdAt: 1,
      updatedAt: 1,
      members: [
        { documentId: "w:a.md", relativePath: "a.md", missing: false },
        { documentId: "w:b.md", relativePath: "b.md", missing: true },
      ],
    };
    expect(membersLabel(collection)).toBe("1 file, 1 missing");
    expect(
      membersLabel({ ...collection, members: [collection.members[0]!] }),
    ).toBe("1 file");
  });
});
