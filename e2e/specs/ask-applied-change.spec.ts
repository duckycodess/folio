import { addFolder, expect, test } from "../support/app";
import { openView } from "../support/ui";

/**
 * A change Olio proposed and the user applied stops offering its preview:
 * the old name no longer exists, so "Preview change…" could only fail. The
 * fake stands in for the model with a canned rename.
 */
test.describe("Ask & Act after applying a change", () => {
  test.use({
    fakeOptions: {
      interpretRename: {
        path: "notes/paalala.md",
        destination: "notes/reminders.md",
      },
    },
  });

  test("shows the rename as done instead of offering it again", async ({
    folio,
  }) => {
    await addFolder(folio);
    await openView(folio, "Ask & Act");
    await folio
      .getByRole("textbox", { name: "Your request", exact: true })
      .fill("rename paalala.md to reminders.md");
    await folio.getByRole("button", { name: "Ask Olio", exact: true }).click();

    const log = folio.locator(".ask-log");
    await expect(log).toContainText(
      "Rename notes/paalala.md to notes/reminders.md",
    );
    await log.getByRole("button", { name: "Preview change…" }).click();
    const dialog = folio.getByRole("dialog");
    await dialog
      .getByRole("button", { name: "Approve and apply this change" })
      .click();
    await dialog.getByRole("button", { name: "Done" }).click();

    await expect(log).toContainText(
      "Done: Renamed notes/paalala.md to notes/reminders.md",
    );
    await expect(
      log.getByRole("button", { name: "Preview change…" }),
    ).toHaveCount(0);
    await expect(
      log.getByRole("button", { name: "Open reminders.md" }),
    ).toBeVisible();
  });
});

test.describe("Conversations", () => {
  test("lists conversations in a dialog where they can be renamed and opened", async ({
    folio,
  }) => {
    await addFolder(folio);
    await openView(folio, "Ask & Act");
    const request = folio.getByRole("textbox", {
      name: "Your request",
      exact: true,
    });
    const ask = folio.getByRole("button", { name: "Ask Olio", exact: true });
    await request.fill("first question");
    await ask.click();
    await expect(folio.locator(".ask-request").first()).toBeVisible();

    await folio.getByRole("button", { name: "Conversations" }).click();
    const dialog = folio.getByRole("dialog", { name: "Conversations" });
    await dialog.getByRole("button", { name: "New conversation" }).click();
    await expect(dialog).toBeHidden();
    await request.fill("second question");
    await ask.click();
    await expect(folio.locator(".ask-request")).toHaveText("second question");

    await folio.getByRole("button", { name: "Conversations" }).click();
    await expect(dialog.getByText("Open now")).toBeVisible();
    await dialog
      .getByRole("button", { name: "Rename conversation: first question" })
      .click();
    await dialog
      .getByRole("textbox", { name: "Conversation name" })
      .fill("Thesis questions");
    await dialog.getByRole("button", { name: "Save name" }).click();
    await expect(
      dialog.getByRole("button", {
        name: "Open conversation: Thesis questions",
      }),
    ).toBeVisible();

    await dialog
      .getByRole("button", { name: "Open conversation: Thesis questions" })
      .click();
    await expect(dialog).toBeHidden();
    await expect(folio.locator(".ask-request")).toHaveText("first question");
  });
});
