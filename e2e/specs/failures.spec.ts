import { RECOVERY } from "../../src/app/recovery";
import { FOLIO_ERROR_CODES } from "../../src/domain/contracts";
import { addFolder, analyzeFolder, expect, fake, test } from "../support/app";
import { fileRow, rowMenu } from "../support/ui";

const PLAN = "projects/project-plan.md";
const RENAMED = "projects/community-learning-project.md";
const CHECKLIST = "projects/submission-checklist.md";

/**
 * Failures and recovery. Every code on the frozen boundary can be injected into
 * the fake core, and the writer can fail partway or report a change it could
 * not record. The point of each case is that the screen never claims more than
 * the reply allows.
 */
test.describe("Failures and recovery", () => {
  test("shows the right wording for every error code on the boundary", async ({
    folio,
  }) => {
    await addFolder(folio);
    for (const code of FOLIO_ERROR_CODES) {
      await fake(folio).failNext("read_document", code);
      await fileRow(folio, PLAN).click();
      await expect(folio.locator(".notice .notice-title").first()).toHaveText(
        RECOVERY[code].title,
      );
      await folio.getByRole("button", { name: "Dismiss" }).first().click();
    }
  });

  test("names the files it could not identify instead of dropping them", async ({
    folio,
  }) => {
    await fake(folio).setSkipped([
      { displayName: "notes/?-invalid-name", code: "pathUnsupportedEncoding" },
    ]);
    await addFolder(folio);
    await expect(
      folio
        .locator(".notice-body")
        .getByText("1 file(s) couldn't be identified and aren't listed."),
    ).toBeVisible();
  });

  test("keeps earlier changes when an operation fails partway", async ({
    folio,
  }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    const boxes = folio.getByRole("checkbox");
    for (const index of [0, 1, 2]) await boxes.nth(index).check();
    await fake(folio).setWriter({
      failAtIndex: 1,
      failCode: "destinationExists",
    });
    await folio.getByRole("button", { name: "Preview 3 changes" }).click();
    await folio
      .getByRole("button", { name: "Approve and apply 3 changes" })
      .click();

    await expect(
      folio.getByRole("heading", { name: /^Saved 1 of 3 changes\./ }),
    ).toBeVisible();
    await expect(
      folio
        .locator(".notice-body")
        .getByText("Earlier changes were kept. You can undo them below.", {
          exact: false,
        }),
    ).toBeVisible();
    const outcomes = folio.getByRole("table").last();
    await expect(
      outcomes.getByRole("cell", { name: "Saved", exact: true }),
    ).toHaveCount(1);
    await expect(outcomes.getByRole("cell", { name: /Not saved/ })).toHaveCount(
      1,
    );
    await expect(
      outcomes.getByRole("cell", { name: "Not started", exact: true }),
    ).toHaveCount(1);
    // What was kept can still be undone.
    await expect(
      folio.getByRole("button", { name: "Preview Undo" }),
    ).toBeVisible();
  });

  test("says a change was saved without a way to undo it", async ({
    folio,
  }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    await folio
      .getByRole("checkbox", { name: new RegExp(PLAN.replace("/", "\\/")) })
      .check();
    await fake(folio).setWriter({ historyRequiredAtIndex: 0 });
    await folio.getByRole("button", { name: "Preview 1 change" }).click();
    await folio
      .getByRole("button", { name: "Approve and apply this change" })
      .click();

    await expect(
      folio
        .locator(".notice-body")
        .getByText(
          "Folio saved the change but couldn't record how to reverse it.",
          { exact: false },
        ),
    ).toBeVisible();
    // The file really did change, so Undo is not offered for it.
    expect(await fake(folio).read(RENAMED)).not.toBeNull();
    await expect(
      folio.getByRole("button", { name: "Preview Undo" }),
    ).toHaveCount(0);
  });

  test("refuses an expired preview and changes nothing", async ({ folio }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    await folio
      .getByRole("checkbox", { name: new RegExp(PLAN.replace("/", "\\/")) })
      .check();
    await folio.getByRole("button", { name: "Preview 1 change" }).click();
    await fake(folio).expirePlans();
    await folio
      .getByRole("button", { name: "Approve and apply this change" })
      .click();

    await expect(folio.locator(".notice-title").first()).toHaveText(
      "This preview has expired",
    );
    await expect(
      folio.getByRole("button", { name: "Preview again" }),
    ).toBeVisible();
    expect(await fake(folio).read(PLAN)).not.toBeNull();
    expect(await fake(folio).read(RENAMED)).toBeNull();
  });

  test("refuses a rename onto an existing file and keeps that file", async ({
    folio,
  }) => {
    await addFolder(folio);
    const before = await fake(folio).read(CHECKLIST);
    await rowMenu(folio, PLAN).click();
    await folio.getByRole("menuitem", { name: "Rename…" }).click();
    const dialog = folio.getByRole("dialog", {
      name: "Rename project-plan.md",
    });
    await dialog.getByLabel("New name").fill("submission-checklist.md");
    await dialog.getByRole("button", { name: "Preview rename" }).click();

    await expect(dialog.locator(".notice-title")).toHaveText(
      "A file with that name already exists",
    );
    await expect(
      dialog.getByRole("button", { name: "Choose another name" }),
    ).toBeVisible();
    expect(await fake(folio).read(CHECKLIST)).toBe(before);
    expect(await fake(folio).read(PLAN)).not.toBeNull();
  });

  test("refuses the whole Undo when a file changed afterwards", async ({
    folio,
  }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    await folio
      .getByRole("checkbox", { name: new RegExp(PLAN.replace("/", "\\/")) })
      .check();
    await folio.getByRole("button", { name: "Preview 1 change" }).click();
    await folio
      .getByRole("button", { name: "Approve and apply this change" })
      .click();
    await expect(
      folio.getByRole("heading", { name: "Saved 1 change." }),
    ).toBeVisible();

    await fake(folio).externalEdit(RENAMED, "# Someone else edited this\n");
    await folio.getByRole("button", { name: "Preview Undo" }).click();
    const modal = folio.getByRole("dialog", { name: "Undo these changes?" });
    await expect(
      modal.getByText("Folio can't undo safely, so nothing will be changed:"),
    ).toBeVisible();
    await expect(
      modal.getByText(`${RENAMED} was changed after Folio saved it.`),
    ).toBeVisible();
    await expect(
      modal.getByRole("button", { name: /^Undo 1 change/ }),
    ).toBeDisabled();
    // The newer edit survived and the old name was not restored.
    expect(await fake(folio).read(RENAMED)).toBe(
      "# Someone else edited this\n",
    );
    expect(await fake(folio).read(PLAN)).toBeNull();
  });
});
