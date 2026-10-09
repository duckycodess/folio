import { expect, test } from "../support/app";
import { openView } from "../support/ui";

const TAGLISH =
  "Hanapin yung project plan at palitan ang deadline na October 20 to October 23.";

/**
 * Journey C — Ask and Act, as far as the merged UI goes today. There is no
 * local model in this build, so a request ends at setup guidance with the
 * instruction kept. Nothing here pretends an assistant exists.
 */
test.describe("Journey C: ask and act", () => {
  test("keeps a Taglish instruction and asks for model setup", async ({
    folio,
  }) => {
    await openView(folio, "Ask & Act");
    const instruction = folio.getByLabel("What should Folio do?");
    await instruction.fill(TAGLISH);
    await folio.getByRole("button", { name: "Preview actions" }).click();

    const notice = folio.locator(".notice-warning");
    await expect(notice.locator(".notice-title")).toHaveText(
      "This needs a local AI model",
    );
    await expect(
      notice.getByText("No model is set up yet. Your request is kept."),
    ).toBeVisible();

    await notice.getByRole("button", { name: "Open Model Lab" }).click();
    await expect(folio.getByText("No local AI model is set up")).toBeVisible();
    await expect(
      folio.getByText(
        "Browsing, keyword search and reading files work without one.",
        {
          exact: false,
        },
      ),
    ).toBeVisible();

    // The instruction survived the trip to Model Lab.
    await openView(folio, "Ask & Act");
    await expect(folio.getByLabel("What should Folio do?")).toHaveValue(
      TAGLISH,
    );
  });

  test("keeps Search, Organize and Summarize reachable without the assistant", async ({
    folio,
  }) => {
    // Journeys A and B have their own entry points; the app is not a chat.
    for (const view of ["Home", "Organize", "Graph", "Ask & Act", "Model Lab"])
      await expect(
        folio.getByRole("button", { name: view, exact: true }),
      ).toBeVisible();
    await openView(folio, "Home");
    await expect(
      folio.getByRole("searchbox", { name: "Search files" }),
    ).toHaveCount(0);
    // Search appears with files to search; Summarize lives on the file itself.
    await folio.getByRole("button", { name: "Look at sample files" }).click();
    await expect(
      folio.getByRole("searchbox", { name: "Search files" }),
    ).toBeVisible();
  });
});
