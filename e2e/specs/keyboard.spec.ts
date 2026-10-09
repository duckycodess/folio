import { addFolder, expect, test } from "../support/app";
import { fileRow, openView, reader, rowMenu, tabUntil } from "../support/ui";

const PLAN = "projects/project-plan.md";

/**
 * Keyboard-only paths, where focus goes, and what is announced. These are the
 * parts of the UI a pointer never exercises.
 */
test.describe("Keyboard and announcements", () => {
  test("adds a folder and opens a file without a pointer at all", async ({
    folio,
  }) => {
    // From the page as it loads: Tab to the only primary action, take it, then
    // Tab on to the file list and open a file. No click, no programmatic focus.
    await tabUntil(folio, 'button:text-is("Add folder")');
    await folio.keyboard.press("Enter");
    await expect(folio.locator(".topbar-badge")).toContainText(
      "Community Learning Project",
    );

    await tabUntil(folio, ".file-list .list-row");
    const opened = await folio.evaluate(
      () => document.activeElement?.getAttribute("data-document-id") ?? "",
    );
    expect(opened).toContain("e2e-workspace:");
    await folio.keyboard.press("Enter");
    await expect(folio.locator(".document-panel")).toBeVisible();

    // Space works on the row too, and Escape comes back to it.
    await folio.keyboard.press("Escape");
    await expect(folio.locator(".document-panel")).toHaveCount(0);
    await folio.keyboard.press("Space");
    await expect(folio.locator(".document-panel")).toBeVisible();
  });

  test("moves between file rows and opens one with the keyboard", async ({
    folio,
  }) => {
    await addFolder(folio);
    const first = folio.locator(".file-list .list-row").first();
    await first.focus();
    await folio.keyboard.press("ArrowDown");
    await folio.keyboard.press("ArrowDown");
    await folio.keyboard.press("Home");
    await expect(first).toBeFocused();
    await folio.keyboard.press("End");
    await expect(folio.locator(".file-list .list-row").last()).toBeFocused();

    // The column headings are hidden from assistive tech, so the row says
    // which value is the date in its own spoken name.
    await expect(fileRow(folio, PLAN)).toHaveAccessibleName(
      /project-plan\.md .*(modified \S|no modified date)/,
    );

    await fileRow(folio, PLAN).focus();
    await folio.keyboard.press("Enter");
    await expect(reader(folio, "project-plan.md")).toBeVisible();
    // Escape closes the reader and gives focus back to the row that opened it.
    await folio.keyboard.press("Escape");
    await expect(reader(folio, "project-plan.md")).toHaveCount(0);
    await expect(fileRow(folio, PLAN)).toBeFocused();
  });

  test("reaches search from another view with the keyboard shortcut", async ({
    folio,
  }) => {
    await addFolder(folio);
    await openView(folio, "Graph");
    await folio.keyboard.press("Control+k");
    await expect(
      folio.getByRole("searchbox", { name: "Search files" }),
    ).toBeFocused();
  });

  test("returns focus to the row's menu when its dialog closes", async ({
    folio,
  }) => {
    await addFolder(folio);
    const menu = rowMenu(folio, PLAN);
    await menu.click();
    await folio.getByRole("menuitem", { name: "Rename…" }).click();

    const dialog = folio.getByRole("dialog", {
      name: "Rename project-plan.md",
    });
    await expect(dialog.getByLabel("New name")).toBeFocused();
    // Tab stays inside the dialog rather than leaving for the page behind it.
    for (let press = 0; press < 6; press += 1)
      await folio.keyboard.press("Tab");
    await expect(dialog.locator(":focus")).toHaveCount(1);

    await folio.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(menu).toBeFocused();
  });

  test("announces what changed through the page's live region", async ({
    folio,
  }) => {
    await addFolder(folio);
    await expect(
      folio.locator('[role="status"]', { hasText: "16 files are listed" }),
    ).toBeVisible();

    await folio.getByRole("searchbox", { name: "Search files" }).fill("tala");
    await expect(
      folio.locator('[role="status"]', {
        hasText: "2 files match your search.",
      }),
    ).toBeVisible();
  });

  test("keeps one skip link and a reachable main region", async ({ folio }) => {
    await folio.keyboard.press("Tab");
    await expect(
      folio.getByRole("link", { name: "Skip to content" }),
    ).toBeFocused();
    await folio.keyboard.press("Enter");
    await expect(folio.locator("#main")).toBeVisible();
  });
});
