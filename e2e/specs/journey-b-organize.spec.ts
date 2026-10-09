import { addFolder, analyzeFolder, expect, fake, test } from "../support/app";
import { fileRow, openView } from "../support/ui";

const PLAN = "projects/project-plan.md";
const RENAMED = "projects/community-learning-project.md";

/**
 * Journey B — Smart Organize: analyze a folder, choose suggestions, review the
 * exact native plan, approve it, and undo it. The fake native core issues the
 * plan identity and digest and owns every write, so nothing here could change a
 * file without an approval that echoes the digest the screen showed.
 */
test.describe("Journey B: smart organize", () => {
  test("reports exact duplicates as evidence, never as a change", async ({
    folio,
  }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    await expect(
      folio.getByText(
        "2 identical copies: archive/project-plan-copy.md, projects/project-plan.md",
      ),
    ).toBeVisible();
    await expect(
      folio.getByText("Folio doesn't move or delete them", { exact: false }),
    ).toBeVisible();
    // Both copies are still exactly where they were.
    expect(await fake(folio).list()).toContain("archive/project-plan-copy.md");
    expect(await fake(folio).list()).toContain(PLAN);
  });

  test("previews, approves, applies and undoes a rename", async ({ folio }) => {
    await addFolder(folio);
    await analyzeFolder(folio);

    await folio
      .getByRole("checkbox", { name: new RegExp(PLAN.replace("/", "\\/")) })
      .check();
    await folio.getByRole("button", { name: "Preview 1 change" }).click();

    await expect(
      folio.getByRole("heading", {
        name: "Exact preview: nothing has changed yet",
      }),
    ).toBeFocused();
    const preview = folio.getByRole("table");
    await expect(preview.getByRole("cell", { name: "Rename" })).toBeVisible();
    await expect(preview.getByRole("cell", { name: PLAN })).toBeVisible();
    await expect(preview.getByRole("cell", { name: RENAMED })).toBeVisible();
    // A preview is not a saved file: nothing has moved yet.
    expect(await fake(folio).read(PLAN)).not.toBeNull();
    expect(await fake(folio).read(RENAMED)).toBeNull();

    await folio
      .getByRole("button", { name: "Approve and apply this change" })
      .click();
    await expect(
      folio.getByRole("heading", { name: "Saved 1 change." }),
    ).toBeVisible();
    expect(await fake(folio).read(PLAN)).toBeNull();
    expect(await fake(folio).read(RENAMED)).toContain(
      "The project submission deadline is October 20.",
    );
    await expect(folio.getByText("Recorded in history: 1 entry")).toBeVisible();

    // The approval came after the preview, and only once.
    const calls = await fake(folio).calls();
    expect(calls.indexOf("approve_plan")).toBeGreaterThan(
      calls.indexOf("prepare_plan"),
    );
    expect(calls.indexOf("apply_plan")).toBeGreaterThan(
      calls.indexOf("approve_plan"),
    );

    await folio.getByRole("button", { name: "Preview Undo" }).click();
    await expect(
      folio.getByRole("dialog", { name: "Undo these changes?" }),
    ).toBeVisible();
    await expect(
      folio.getByText("Nothing changes until you confirm"),
    ).toBeVisible();
    await folio.getByRole("button", { name: "Undo 1 change" }).click();
    await expect(folio.getByText("Undid 1 change.")).toBeVisible();
    expect(await fake(folio).read(PLAN)).not.toBeNull();
    expect(await fake(folio).read(RENAMED)).toBeNull();

    // The folder listing caught up with the reversal.
    await openView(folio, "Home");
    await expect(fileRow(folio, PLAN)).toBeVisible();
    await expect(fileRow(folio, RENAMED)).toHaveCount(0);
  });

  test("renames one file from its row and shows the same exact preview", async ({
    folio,
  }) => {
    await addFolder(folio);
    await folio
      .locator(`[data-row-id="e2e-workspace:${PLAN}"] .row-menu-button`)
      .click();
    await folio.getByRole("menuitem", { name: "Rename…" }).click();

    const dialog = folio.getByRole("dialog", {
      name: "Rename project-plan.md",
    });
    await expect(dialog).toBeVisible();
    await dialog.getByLabel("New name").fill("deadline-plan.md");
    await dialog.getByRole("button", { name: "Preview rename" }).click();

    await expect(
      dialog.getByRole("heading", {
        name: "Exact preview: nothing has changed yet",
      }),
    ).toBeVisible();
    expect(await fake(folio).read(PLAN)).not.toBeNull();

    await dialog
      .getByRole("button", { name: "Approve and apply this change" })
      .click();
    await expect(
      dialog.getByRole("heading", { name: "Saved 1 change." }),
    ).toBeVisible();
    expect(await fake(folio).read("projects/deadline-plan.md")).not.toBeNull();
    expect(await fake(folio).read(PLAN)).toBeNull();
  });
});
