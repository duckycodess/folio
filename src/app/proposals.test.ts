import { describe, expect, it } from "vitest";
import type { OperationProposal } from "../domain/contracts";
import { changedRegion, proposalOperation } from "./proposals";

const RENAME: OperationProposal = {
  kind: "rename",
  documentId: "w:a.md",
  relativePath: "a.md",
  observedContentHash: "sha256:1",
  destinationRelativePath: "b.md",
};

describe("proposal operations", () => {
  it("pins the revision Olio read and refuses to overwrite", () => {
    expect(proposalOperation(RENAME)).toEqual({
      kind: "rename",
      documentId: "w:a.md",
      relativePath: "a.md",
      expectedContentHash: "sha256:1",
      destinationRelativePath: "b.md",
      expectedDestination: "absent",
    });
    expect(proposalOperation({ ...RENAME, kind: "move" })?.kind).toBe("move");
  });

  it("creates Markdown or plain text by the new file's extension", () => {
    const create = (path: string) =>
      proposalOperation({
        kind: "create",
        destinationRelativePath: path,
        content: "x",
      });
    expect(create("notes/new.md")).toMatchObject({
      mediaType: "text/markdown",
      expectedDestination: "absent",
    });
    expect(create("notes/new.TXT")).toMatchObject({ mediaType: "text/plain" });
  });

  it("leaves edits to the native core", () => {
    expect(
      proposalOperation({
        kind: "edit",
        documentId: "w:a.md",
        relativePath: "a.md",
        observedContentHash: "sha256:1",
        find: "October 20",
        replace: "October 23",
        targetEvidence: {
          documentId: "w:a.md",
          documentContentHash: "sha256:1",
          offsetUnit: "utf8Byte",
          start: 0,
          end: 10,
          text: "October 20",
        },
      }),
    ).toBeNull();
  });
});

describe("changed region", () => {
  it("shows exactly what changes, with context and its line", () => {
    const before = "# Plan\n\nDeadline: October 20.\nOwner: Maya\n";
    const after = "# Plan\n\nDeadline: October 23.\nOwner: Maya\n";
    expect(changedRegion(before, after, 10)).toEqual({
      line: 3,
      before: " October 2",
      removed: "0",
      added: "3",
      after: ".\nOwner: M",
      clippedStart: true,
      clippedEnd: true,
    });
  });

  it("handles insertions, deletions and no change", () => {
    expect(changedRegion("ab", "aXb")).toMatchObject({
      removed: "",
      added: "X",
    });
    expect(changedRegion("aXb", "ab")).toMatchObject({
      removed: "X",
      added: "",
    });
    expect(changedRegion("aaa", "aaaa")).toMatchObject({
      removed: "",
      added: "a",
    });
    expect(changedRegion("same", "same")).toBeNull();
  });
});
