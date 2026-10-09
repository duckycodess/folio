import { addFolder, expect, fake, test } from "../support/app";
import { fileRow, reader } from "../support/ui";

/**
 * Search and Local Sync: what search covers before a folder is indexed, the
 * evidence it shows afterwards, and what Stop really means. Every result here
 * comes from the fake core's `search_index`, whose passages are located at real
 * UTF-8 byte offsets in the fixture text.
 */
test.describe("Search and indexing", () => {
  async function search(page: Parameters<typeof addFolder>[0], query: string) {
    await page.getByRole("searchbox", { name: "Search files" }).fill(query);
  }

  test("searches names only until the folder is indexed, then its text", async ({
    folio,
  }) => {
    await addFolder(folio);
    await search(folio, "panayam");

    await expect(
      folio.getByText(
        "Only file names are searched until Folio indexes this folder.",
        {
          exact: false,
        },
      ),
    ).toBeVisible();
    await expect(folio.getByText("No matching files")).toBeVisible();

    await folio.getByRole("button", { name: "Index this folder" }).click();
    await expect(
      folio.getByRole("button", { name: "Index this folder" }),
    ).toHaveCount(0);

    // Two documents contain the Filipino word; both are listed with evidence.
    await expect(fileRow(folio, "notes/tala-sa-proyekto.md")).toBeVisible();
    await expect(
      fileRow(folio, "research/tala-sa-pamamaraan.md"),
    ).toBeVisible();
    await expect(folio.getByText("Words in the text").first()).toBeVisible();
    await expect(folio.locator(".result-evidence mark").first()).toHaveText(
      "panayam",
    );
    await expect(
      folio.locator('[role="status"]', { hasText: "files match your search" }),
    ).toBeVisible();
    // Keyword matching is never presented as search by meaning.
    await expect(folio.getByText("Keyword search").first()).toBeVisible();
  });

  test("opens the reader at a search excerpt, highlighted", async ({
    folio,
  }) => {
    await addFolder(folio);
    await search(folio, "panayam");
    await folio.getByRole("button", { name: "Index this folder" }).click();
    await expect(fileRow(folio, "notes/tala-sa-proyekto.md")).toBeVisible();

    await folio
      .getByRole("button", {
        name: /^Open tala-sa-proyekto\.md at this passage/,
      })
      .first()
      .click();
    const panel = reader(folio, "tala-sa-proyekto.md");
    await expect(panel.locator("mark.source-highlight")).toContainText(
      "May 12 boluntaryong estudyante sa panayam.",
    );
  });

  test("finds a text PDF's page", async ({ folio }) => {
    await addFolder(folio);
    await search(folio, "consent");
    await folio.getByRole("button", { name: "Index this folder" }).click();
    await expect(
      fileRow(folio, "research/consent-form-guide.pdf"),
    ).toBeVisible();
    await expect(folio.getByText("Page 1 ·").first()).toBeVisible();
  });

  test("Stop ends the scan with a cancelled summary and keeps names-only search", async ({
    folio,
  }) => {
    await addFolder(folio);
    // Slow enough that Stop lands before the first file is committed.
    await fake(folio).setScanStepMs(1500);
    await search(folio, "panayam");
    await folio.getByRole("button", { name: "Index this folder" }).click();
    await folio.getByRole("button", { name: "Stop" }).click();

    // The reply itself says the scan was stopped; it is not a failure.
    await expect
      .poll(async () => (await fake(folio).lastScan())?.cancelled)
      .toBe(true);
    expect((await fake(folio).lastScan())?.total).toBe(0);
    await expect(
      folio.getByText(
        "Only file names are searched until Folio indexes this folder.",
        {
          exact: false,
        },
      ),
    ).toBeVisible();
    await expect(folio.getByRole("button", { name: "Stop" })).toHaveCount(0);
  });
});
