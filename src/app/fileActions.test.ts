import { describe, expect, it } from "vitest";
import type { DocumentRecord } from "../domain/contracts";
import {
  fileActionAvailability,
  moveFolders,
  renameProblem,
} from "./fileActions";
import { moveOperation, renameOperation } from "./useOrganize";

const folder = { source: "folder", nativeAvailable: true } as const;
const markdown = { mediaType: "text/markdown" } as const;
const pdf = { mediaType: "application/pdf" } as const;

describe("file action availability", () => {
  it("needs the desktop app and a folder the user added", () => {
    expect(
      fileActionAvailability(
        { source: "samples", nativeAvailable: false },
        markdown,
      ),
    ).toMatchObject({ available: false, reason: /desktop app/ });
    expect(
      fileActionAvailability(
        { source: "samples", nativeAvailable: true },
        markdown,
      ),
    ).toMatchObject({ available: false, reason: /Sample files/ });
    expect(fileActionAvailability(folder, markdown)).toEqual({
      available: true,
    });
  });

  it("only opens PDFs", () => {
    expect(fileActionAvailability(folder, pdf)).toMatchObject({
      available: false,
      reason: /PDFs can only be opened/,
    });
  });
});

describe("rename names", () => {
  it("accepts a new text or Markdown name, in English or Filipino", () => {
    expect(renameProblem("notes/plan.md", "plano-ng-proyekto.md")).toBeNull();
    expect(renameProblem("notes/plan.md", "  Talaan ni Niña.txt ")).toBeNull();
  });

  it("explains a name the native core would refuse", () => {
    expect(renameProblem("plan.md", " ")).toMatch(/Type a new name/);
    expect(renameProblem("plan.md", "a/b.md")).toMatch(/can't contain/);
    expect(renameProblem("plan.md", "a\\b.md")).toMatch(/can't contain/);
    expect(renameProblem("notes/Plan.md", "plan.MD")).toMatch(/current name/);
    expect(renameProblem("plan.md", "plan.pdf")).toMatch(/\.md/);
    expect(renameProblem("plan.md", "plan")).toMatch(/\.md/);
  });
});

describe("move destinations", () => {
  const documents = [
    { relativePath: "plan.md" },
    { relativePath: "projects/2026/plan.md" },
    { relativePath: "projects/budget.md" },
    { relativePath: "Archive/Plan.md" },
    { relativePath: "notes/tala.txt" },
  ];

  it("offers only existing folders, never the file's own, and flags name clashes", () => {
    expect(moveFolders(documents, { relativePath: "notes/tala.txt" })).toEqual([
      { folder: "" },
      { folder: "Archive" },
      { folder: "projects" },
      { folder: "projects/2026" },
    ]);
    expect(
      moveFolders(documents, { relativePath: "projects/budget.md" }),
    ).toEqual([
      { folder: "" },
      { folder: "Archive" },
      { folder: "notes" },
      { folder: "projects/2026" },
    ]);
    // `plan.md` is already in the top folder, in projects/2026 and (in
    // another case) in Archive.
    const plan = moveFolders(documents, {
      relativePath: "projects/2026/plan.md",
    });
    expect(
      plan.filter((item) => item.blocked).map((item) => item.folder),
    ).toEqual(["", "Archive"]);
  });
});

describe("rename and move operations", () => {
  const document = {
    id: "w:projects/plan.md",
    relativePath: "projects/plan.md",
    contentHash: `sha256:${"a".repeat(64)}`,
  } as DocumentRecord & { contentHash: string };

  it("pin the revision and never replace an existing file", () => {
    expect(moveOperation(document, "archive/2026")).toEqual({
      kind: "move",
      documentId: document.id,
      relativePath: "projects/plan.md",
      expectedContentHash: document.contentHash,
      destinationRelativePath: "archive/2026/plan.md",
      expectedDestination: "absent",
    });
    expect(moveOperation(document, "")).toMatchObject({
      destinationRelativePath: "plan.md",
    });
    expect(renameOperation(document, "plano.md")).toMatchObject({
      kind: "rename",
      destinationRelativePath: "projects/plano.md",
      expectedDestination: "absent",
    });
  });
});
