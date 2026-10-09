import { addFolder, expect, fake, test } from "../support/app";
import { RECOVERY } from "../../src/app/recovery";
import { openView } from "../support/ui";

const TAGLISH =
  "Hanapin yung project plan at palitan ang deadline na October 20 to October 23.";

/**
 * Journey C — Ask and Act, as far as the merged UI goes today. There is no
 * installed model in the fake, so the real assistant UI retains a failed
 * request and offers setup. This does not simulate a model-generated reply.
 */
test.describe("Journey C: ask and act", () => {
  test("keeps a Taglish instruction and asks for model setup", async ({
    folio,
  }) => {
    await addFolder(folio);
    await openView(folio, "Ask & Search");
    const instruction = folio.getByRole("textbox", {
      name: "Your request",
      exact: true,
    });
    await instruction.fill(TAGLISH);
    await folio.getByRole("button", { name: "Ask Olio", exact: true }).click();

    const notice = folio.locator(".notice-warning");
    await expect(notice.locator(".notice-title")).toHaveText(
      RECOVERY.modelNotInstalled.title,
    );
    await expect(folio.locator(".ask-request")).toHaveText(TAGLISH);
    expect(await fake(folio).calls()).toContain("interpret_request");

    await notice.getByRole("button", { name: "Open Model Lab" }).click();
    await expect(
      folio.getByRole("heading", { name: "Model Lab", exact: true }),
    ).toBeVisible();
    await expect(
      folio.getByRole("button", { name: /^Download / }).first(),
    ).toBeVisible();

    // The instruction survived the trip to Model Lab.
    await openView(folio, "Ask & Search");
    await expect(
      folio.getByRole("textbox", { name: "Your request", exact: true }),
    ).toHaveValue(TAGLISH);
  });

  test("keeps Search, Organize and Summarize reachable without the assistant", async ({
    folio,
  }) => {
    // Journeys A and B have their own entry points; the app is not a chat.
    for (const view of [
      "Home",
      "Organize",
      "Graph",
      "Ask & Search",
      "Activity",
      "Settings & style",
    ])
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
