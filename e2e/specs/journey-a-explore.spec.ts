import { addFolder, analyzeFolder, expect, test } from "../support/app";
import { fileRow, openFile, openView, reader } from "../support/ui";

/**
 * Journey A — Explore and Understand: browse or search, select a file, read it,
 * and follow its connections. Everything here is driven through the merged UI
 * against the fake native core's real `list_documents`, `read_document`,
 * `list_relationships` and `list_duplicates` replies.
 */
test.describe("Journey A: explore and understand", () => {
  test("adds a folder and lists its documents with their locations", async ({
    folio,
  }) => {
    await expect(folio.getByText("Add a folder to get started")).toBeVisible();
    await addFolder(folio);

    await expect(folio.locator(".workspace-source-detail")).toHaveText(
      "/Users/folio/Documents/Community Learning Project",
    );
    await expect(fileRow(folio, "projects/project-plan.md")).toBeVisible();
    await expect(fileRow(folio, "notes/tala-sa-proyekto.md")).toBeVisible();
    await expect(
      fileRow(folio, "research/consent-form-guide.pdf"),
    ).toBeVisible();
    // A row is spoken as the columns it shows: name, location, type, modified
    // and size, with no second wording a voice-control user could not say.
    await expect(
      fileRow(folio, "projects/project-plan.md"),
    ).toHaveAccessibleName(/project-plan\.md.*projects.*Markdown.*B$/s);
  });

  test("reads a Filipino document and keeps it read-only", async ({
    folio,
  }) => {
    await addFolder(folio);
    const panel = await openFile(folio, "notes/tala-sa-proyekto.md");
    await expect(panel.getByRole("tab", { name: "Details" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(panel.getByText("Read-only")).toBeVisible();
    await expect(
      panel.getByText(
        "Ang huling araw ng pagpasa ng Community Learning Project",
      ),
    ).toBeVisible();
    await expect(panel.getByText("notes/tala-sa-proyekto.md")).toBeVisible();
  });

  test("says what it cannot see before the folder is indexed", async ({
    folio,
  }) => {
    await addFolder(folio);
    const panel = await openFile(folio, "meetings/meeting-notes.md");
    await panel.getByRole("tab", { name: "Related" }).click();
    await expect(
      panel.getByText("This folder hasn't been indexed yet", { exact: false }),
    ).toBeVisible();
    await expect(panel.getByText("No related files found")).toBeVisible();
  });

  test("follows a document's connections and comes back", async ({ folio }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    await openView(folio, "Home");
    const panel = await openFile(folio, "meetings/meeting-notes.md");
    await panel.getByRole("tab", { name: "Related" }).click();
    const link = panel
      .getByRole("button", { name: /project-plan\.md/ })
      .first();
    await expect(link).toBeVisible();
    await link.click();

    const opened = reader(folio, "project-plan.md");
    await expect(opened).toBeVisible();
    const back = opened.getByRole("button", {
      name: "Back to meeting-notes.md",
    });
    await expect(back).toBeVisible();
    await back.click();
    await expect(
      reader(folio, "meeting-notes.md").getByRole("tab", { name: "Related" }),
    ).toHaveAttribute("aria-selected", "true");
  });

  test("shows the whole folder's connections in Graph, with evidence", async ({
    folio,
  }) => {
    await addFolder(folio);
    // Graph reads the folder index, which Organize's Analyze fills.
    await analyzeFolder(folio);
    await openView(folio, "Graph");
    await folio.getByRole("button", { name: "List", exact: true }).click();
    await expect(
      folio.getByRole("region", { name: "Connections between files" }),
    ).toBeVisible();
    const evidence = folio
      .getByRole("button", { name: /^Show passage: \[project plan\]/ })
      .first();
    await expect(evidence).toBeVisible();
    await evidence.click();

    // The passage opens in the reader, highlighted and focused.
    const panel = reader(folio, "meeting-notes.md");
    await expect(panel.locator("mark.source-highlight")).toHaveText(
      "[project plan](../projects/project-plan.md)",
    );
    await expect(panel.locator("mark.source-highlight")).toBeFocused();
  });
});
