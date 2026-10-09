import type { Page } from "@playwright/test";
import { addFolder, analyzeFolder, expect, fake, test } from "../support/app";
import { openView } from "../support/ui";

const PLAN = "projects/project-plan.md";
const CHECKLIST = "projects/submission-checklist.md";
const NOTES = "meetings/meeting-notes.md";
const RENAMED = "projects/community-learning-project.md";

/** A visible notice, not its copy in the live region. */
function notice(folio: Page, text: string) {
  return folio.locator(".notice-body", { hasText: text });
}

/**
 * Journey B's suggested collections (#78). The fake's groups stand in for the
 * real core's embedding and generation models, so these journeys check how the
 * UI handles suggestions and kept collections, never grouping quality.
 */
test.describe("Journey B: collections without a local model", () => {
  test("says grouping needs an embedding model and suggests none", async ({
    folio,
  }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    await expect(
      notice(folio, "Grouping files by meaning needs a local embedding model"),
    ).toBeVisible();
    await expect(folio.getByText("Name written by local AI")).toHaveCount(0);
  });
});

test.describe("Journey B: suggested collections", () => {
  test.use({
    fakeOptions: {
      collectionGroups: [
        { paths: [PLAN, CHECKLIST, NOTES], name: "Project deadlines" },
        { paths: ["courses/math-review.md", "courses/pagsasanay-sa-math.md"] },
      ],
    },
  });

  test("keeps a named group without moving a file, then follows a rename", async ({
    folio,
  }) => {
    await addFolder(folio);
    const before = await fake(folio).list();
    await analyzeFolder(folio);

    const named = folio.getByLabel("Group 1: collection name");
    await expect(named).toHaveValue("Project deadlines");
    await expect(folio.getByText("Name written by local AI")).toBeVisible();
    // A group the model didn't name can't be kept until it has a name.
    await expect(folio.getByLabel("Group 2: collection name")).toHaveValue("");
    await expect(folio.getByText("Give the collection a name.")).toBeVisible();

    // Leave the meeting notes out, then keep the rest.
    await folio
      .locator(".collection-suggestion")
      .first()
      .getByRole("checkbox", { name: new RegExp(NOTES.replace("/", "\\/")) })
      .uncheck();
    await folio
      .getByRole("button", { name: "Keep collection" })
      .first()
      .click();
    await expect(
      notice(folio, "Kept as “Project deadlines”. No files were moved."),
    ).toBeVisible();
    expect(await fake(folio).list()).toEqual(before);
    const calls = await fake(folio).calls();
    expect(calls).toContain("keep_collection");
    expect(calls).not.toContain("prepare_plan");

    const kept = folio.locator(".collection-item", {
      has: folio.getByRole("heading", { name: "Project deadlines" }),
    });
    await expect(kept.getByText("2 files")).toBeVisible();
    await expect(kept.getByText(PLAN)).toBeVisible();
    await expect(kept.getByText(NOTES)).toHaveCount(0);

    // Rename the plan through the exact preview; the collection follows it.
    await folio
      .getByRole("group", { name: "Name suggestions" })
      .getByRole("checkbox", { name: new RegExp(PLAN.replace("/", "\\/")) })
      .check();
    await folio.getByRole("button", { name: "Preview 1 change" }).click();
    await folio
      .getByRole("button", { name: "Approve and apply this change" })
      .click();
    await expect(
      folio.getByRole("heading", { name: "Saved 1 change." }),
    ).toBeVisible();
    await expect(kept.getByText(RENAMED)).toBeVisible();
    await expect(kept.getByText("Missing", { exact: false })).toHaveCount(0);

    // Home lists the kept collection too.
    await openView(folio, "Home");
    await expect(folio.locator(".home-collection")).toContainText(
      "Project deadlines",
    );
  });

  test("analyzes one collection: suggestions only for its files", async ({
    folio,
  }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    await folio.getByLabel("Group 2: collection name").fill("Math");
    await folio.getByRole("button", { name: "Keep collection" }).nth(1).click();
    await expect(notice(folio, "Kept as “Math”.")).toBeVisible();

    await folio
      .getByRole("button", { name: "Analyze this collection" })
      .click();
    await expect(folio.getByLabel("What to analyze")).toHaveValue(
      /collection-/,
    );
    await folio.getByRole("button", { name: "Analyze", exact: true }).click();
    await expect(
      folio.getByRole("heading", { name: "Exact duplicates" }),
    ).toBeVisible();
    // The plan's identical copy isn't in "Math", and no collection is suggested.
    await expect(
      folio.getByText("No files with identical contents."),
    ).toBeVisible();
    await expect(
      folio.getByRole("checkbox", {
        name: new RegExp(PLAN.replace("/", "\\/")),
      }),
    ).toHaveCount(0);
    await expect(
      folio.getByRole("heading", { name: "Suggested collections" }),
    ).toHaveCount(0);
  });

  test("refuses to keep a group whose file changed since the analysis", async ({
    folio,
  }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    await fake(folio).externalEdit(CHECKLIST, "Changed in another app.");
    await folio
      .getByRole("button", { name: "Keep collection" })
      .first()
      .click();
    await expect(
      notice(folio, "changed since Folio analyzed it"),
    ).toBeVisible();
    await expect(
      folio.getByRole("heading", { name: "Project deadlines" }),
    ).toHaveCount(0);
  });
});

test.describe("Journey B: names and folders from the local AI", () => {
  const BUDGET = "personal/budget-notes.md";
  const STUDY = "notes/study-session.md";
  test.use({
    fakeOptions: {
      collectionGroups: [
        { paths: [PLAN, CHECKLIST], name: "Project deadlines" },
      ],
      modelFilenames: [{ path: BUDGET, name: "Badyet sa Oktubre" }],
      destinations: [{ path: STUDY, folder: "courses" }],
    },
  });

  test("previews a model-written name and a folder move as one exact plan", async ({
    folio,
  }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    const names = folio.getByRole("group", { name: "Name suggestions" });
    const folders = folio.getByRole("group", { name: "Folder suggestions" });
    await expect(names.getByText("Name by local AI")).toBeVisible();
    await names.getByRole("checkbox", { name: /badyet-sa-oktubre/ }).check();
    await folders.getByRole("checkbox", { name: /study-session/ }).check();
    await folio.getByRole("button", { name: "Preview 2 changes" }).click();

    const preview = folio.getByRole("table");
    await expect(preview.getByRole("cell", { name: "Rename" })).toBeVisible();
    await expect(preview.getByRole("cell", { name: "Move" })).toBeVisible();
    await expect(
      preview.getByRole("cell", { name: "courses/study-session.md" }),
    ).toBeVisible();
    // A preview is not a saved file.
    expect(await fake(folio).read(STUDY)).not.toBeNull();

    await folio
      .getByRole("button", { name: "Approve and apply 2 changes" })
      .click();
    await expect(
      folio.getByRole("heading", { name: "Saved 2 changes." }),
    ).toBeVisible();
    expect(await fake(folio).read("courses/study-session.md")).not.toBeNull();
    expect(
      await fake(folio).read("personal/badyet-sa-oktubre.md"),
    ).not.toBeNull();
    expect(await fake(folio).read(STUDY)).toBeNull();
  });

  test("lets only one change per file be chosen", async ({ folio }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    const names = folio.getByRole("group", { name: "Name suggestions" });
    const title = names.getByRole("checkbox", {
      name: /^personal\/budget-notes\.md(?!.*badyet)/,
    });
    const model = names.getByRole("checkbox", { name: /badyet-sa-oktubre/ });
    await expect(
      folio.getByText("Choose one change per file", { exact: false }),
    ).toBeVisible();
    await title.check();
    await model.check();
    await expect(title).not.toBeChecked();
    await expect(
      folio.getByRole("button", { name: "Preview 1 change" }),
    ).toBeVisible();
  });

  test("adds a file to a kept collection from its row, without moving it", async ({
    folio,
  }) => {
    await addFolder(folio);
    await analyzeFolder(folio);
    await folio.getByRole("button", { name: "Keep collection" }).click();
    await expect(notice(folio, "Kept as “Project deadlines”.")).toBeVisible();

    await openView(folio, "Home");
    const before = await fake(folio).list();
    await folio
      .locator(
        `[data-row-id="e2e-workspace:notes/paalala.md"] .row-menu-button`,
      )
      .click();
    await folio.getByRole("menuitem", { name: "Add to collection…" }).click();
    const dialog = folio.getByRole("dialog", {
      name: "Add paalala.md to a collection",
    });
    await expect(
      dialog.getByRole("radio", { name: /Project deadlines/ }),
    ).toBeChecked();
    await dialog.getByRole("button", { name: "Add to collection" }).click();
    await expect(
      dialog.getByText(
        "Added to “Project deadlines”. The file stays where it is.",
      ),
    ).toBeVisible();
    await dialog.getByRole("button", { name: "Done" }).click();
    expect(await fake(folio).list()).toEqual(before);

    await openView(folio, "Organize");
    const kept = folio.locator(".collection-item", {
      has: folio.getByRole("heading", { name: "Project deadlines" }),
    });
    await expect(kept.getByText("3 files")).toBeVisible();
    await expect(kept.getByText("notes/paalala.md")).toBeVisible();
  });
});
