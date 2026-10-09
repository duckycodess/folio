import { describe, expect, it } from "vitest";
import type { Connection } from "./connections";
import type { DocumentRecord } from "./contracts";
import {
  folderSpread,
  graphPairs,
  isConfirmed,
  startingFiles,
} from "./graphScope";
import { WORKSPACE, documentId } from "./test-support";

function file(relativePath: string, content?: string): DocumentRecord {
  const name = relativePath.split("/").at(-1)!;
  return {
    id: documentId(relativePath),
    workspaceId: WORKSPACE,
    relativePath,
    name,
    title: name,
    language: "en",
    mediaType: "text/markdown",
    sizeBytes: content?.length ?? 0,
    content,
    contentHash: content === undefined ? undefined : "c".repeat(64),
  } as DocumentRecord;
}

const NOTES = file("meetings/notes.md", "Interview methods and deadlines");
const PLAN = file("projects/plan.md", "The project deadline is October 23");
const BUDGET = file("projects/budget.md", "Costs only");
const COPY = file("archive/plan-copy.md");
const LONE = file("lone.md", "Nothing connects here");
const ALL = [NOTES, PLAN, BUDGET, COPY, LONE];

const LINK_NOTES_PLAN: Connection = {
  kind: "explicitReference",
  otherId: PLAN.id,
  direction: "outgoing",
  provenance: "documentLink",
  evidence: [],
};
const SIMILAR_PLAN_BUDGET: Connection = {
  kind: "similarity",
  otherId: BUDGET.id,
  direction: "mutual",
  provenance: "embedding",
  evidence: [],
  score: 0.8,
};
const DUPLICATE_PLAN_COPY: Connection = {
  kind: "exactDuplicate",
  otherId: COPY.id,
  direction: "mutual",
  provenance: "contentHash",
  evidence: [],
};

/** Both ends of every connection see it, as `connectionsFor` reports. */
function connectionsOf(id: string): Connection[] {
  const mirror = (c: Connection, otherId: string): Connection => ({
    ...c,
    otherId,
    direction: c.direction === "outgoing" ? "incoming" : c.direction,
  });
  switch (id) {
    case NOTES.id:
      return [LINK_NOTES_PLAN];
    case PLAN.id:
      return [
        mirror(LINK_NOTES_PLAN, NOTES.id),
        DUPLICATE_PLAN_COPY,
        SIMILAR_PLAN_BUDGET,
      ];
    case BUDGET.id:
      return [mirror(SIMILAR_PLAN_BUDGET, PLAN.id)];
    case COPY.id:
      return [mirror(DUPLICATE_PLAN_COPY, PLAN.id)];
    default:
      return [];
  }
}

const names = (pairs: ReturnType<typeof graphPairs>) =>
  pairs.map(({ from, to }) => `${from.name}-${to.name}`);

describe("graphPairs", () => {
  it("lists each connection once for the whole folder", () => {
    expect(names(graphPairs(ALL, connectionsOf, { kind: "all" }))).toEqual([
      "plan-copy.md-plan.md",
      "notes.md-plan.md",
      "budget.md-plan.md",
    ]);
  });

  it("starts from one file, with that file first", () => {
    const pairs = graphPairs(ALL, connectionsOf, {
      kind: "file",
      documentId: PLAN.id,
    });
    expect(names(pairs)).toEqual([
      "plan.md-notes.md",
      "plan.md-plan-copy.md",
      "plan.md-budget.md",
    ]);
    expect(pairs[0].connection.direction).toBe("incoming");
  });

  it("starts from a folder, including connections that leave it", () => {
    expect(
      names(
        graphPairs(ALL, connectionsOf, { kind: "folder", folder: "meetings" }),
      ),
    ).toEqual(["notes.md-plan.md"]);
  });

  it("doesn't treat a folder name as a prefix of another folder", () => {
    expect(
      startingFiles([file("project/a.md"), PLAN], {
        kind: "folder",
        folder: "project",
      }),
    ).toEqual(new Set([documentId("project/a.md")]));
  });

  it("starts from a topic matched in names and read text", () => {
    expect(
      names(
        graphPairs(ALL, connectionsOf, { kind: "topic", term: "interview" }),
      ),
    ).toEqual(["notes.md-plan.md"]);
    expect(
      graphPairs(ALL, connectionsOf, { kind: "topic", term: "  " }),
    ).toEqual([]);
  });

  it("is empty for a file that is no longer listed", () => {
    expect(
      graphPairs(ALL, connectionsOf, { kind: "file", documentId: "gone" }),
    ).toEqual([]);
  });
});

describe("isConfirmed", () => {
  it("confirms links and identical bytes, never similarity", () => {
    expect(isConfirmed(LINK_NOTES_PLAN)).toBe(true);
    expect(isConfirmed(DUPLICATE_PLAN_COPY)).toBe(true);
    expect(isConfirmed(SIMILAR_PLAN_BUDGET)).toBe(false);
    expect(
      isConfirmed({ ...SIMILAR_PLAN_BUDGET, kind: "sharedFactCandidate" }),
    ).toBe(false);
  });
});

describe("folderSpread", () => {
  it("counts each connected file once per folder, most first", () => {
    expect(
      folderSpread(graphPairs(ALL, connectionsOf, { kind: "all" })),
    ).toEqual([
      { folder: "projects", files: 2 },
      { folder: "archive", files: 1 },
      { folder: "meetings", files: 1 },
    ]);
  });
});
