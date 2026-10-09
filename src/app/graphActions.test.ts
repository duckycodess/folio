import { describe, expect, it } from "vitest";
import { deleteOperation, graphActions } from "./graphActions";

const desktopFolder = { source: "folder" as const, nativeAvailable: true };
const markdown = { mediaType: "text/markdown" as const };

describe("Graph node actions", () => {
  it("offers rename, move, edit and delete for a text file in a folder", () => {
    const actions = graphActions(desktopFolder, markdown);
    expect(actions.map((action) => action.kind)).toEqual([
      "rename",
      "move",
      "edit",
      "delete",
    ]);
    expect(actions.every((action) => !action.disabledReason)).toBe(true);
  });

  it("lists every action for a PDF but lets it only be opened", () => {
    const actions = graphActions(desktopFolder, {
      mediaType: "application/pdf",
    });
    expect(actions).toHaveLength(4);
    for (const action of actions)
      expect(action.disabledReason).toMatch(/PDFs can only be opened/);
  });

  it("never offers a change for sample files or in the browser preview", () => {
    for (const action of graphActions(
      { source: "samples", nativeAvailable: true },
      markdown,
    ))
      expect(action.disabledReason).toMatch(/Sample files can't be changed/);
    for (const action of graphActions(
      { source: "samples", nativeAvailable: false },
      markdown,
    ))
      expect(action.disabledReason).toMatch(/desktop app/);
  });
});

describe("deleting from Graph", () => {
  it("pins the revision Folio read, with no destination", () => {
    expect(
      deleteOperation({
        id: "w:notes/paalala.md",
        relativePath: "notes/paalala.md",
        contentHash: "sha256:abc",
      }),
    ).toEqual({
      kind: "delete",
      documentId: "w:notes/paalala.md",
      relativePath: "notes/paalala.md",
      expectedContentHash: "sha256:abc",
    });
  });
});
