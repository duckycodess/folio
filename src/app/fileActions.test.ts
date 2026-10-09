import { describe, expect, it } from "vitest";
import type { DocumentRecord } from "../domain/contracts";
import { folderChoices, nameProblem, relocateOperation } from "./fileActions";

const doc = (relativePath: string) =>
  ({
    id: `w:${relativePath}`,
    relativePath,
    contentHash: "a".repeat(64),
  }) as DocumentRecord & { contentHash: string };

describe("moving a file", () => {
  it("offers every folder inside the open folder, and the top", () => {
    expect(
      folderChoices([
        doc("school/math/review.md"),
        doc("school/notes.md"),
        doc("plan.md"),
      ]),
    ).toEqual(["", "school", "school/math"]);
  });

  it("keeps the name and changes the folder", () => {
    expect(
      relocateOperation(doc("school/notes.md"), { folder: "school/math" }),
    ).toMatchObject({
      kind: "move",
      relativePath: "school/notes.md",
      destinationRelativePath: "school/math/notes.md",
      expectedDestination: "absent",
    });
    expect(
      relocateOperation(doc("school/notes.md"), { folder: "" })
        .destinationRelativePath,
    ).toBe("notes.md");
  });
});

describe("renaming a file", () => {
  it("keeps the folder and pins the revision Folio read", () => {
    expect(
      relocateOperation(doc("school/notes.md"), { name: " tala.md " }),
    ).toMatchObject({
      kind: "rename",
      destinationRelativePath: "school/tala.md",
      expectedContentHash: "a".repeat(64),
    });
  });

  it("refuses names that would leave the folder or change nothing", () => {
    expect(nameProblem("", "a.md")).toMatch(/Type a new name/);
    expect(nameProblem("../x.md", "a.md")).toMatch(/can't contain/);
    expect(nameProblem("x\\y.md", "a.md")).toMatch(/can't contain/);
    expect(nameProblem("..", "a.md")).toMatch(/different name/);
    expect(nameProblem("a.md", "a.md")).toMatch(/already/);
    expect(nameProblem("b.md", "a.md")).toBeNull();
    // Refused natively, since Windows and macOS folders usually ignore case.
    expect(nameProblem("Notes.md", "notes.md")).toMatch(/capital letters/);
    expect(nameProblem(" NOTES.MD ", "notes.md")).toMatch(/capital letters/);
  });
});
