import { addFolder, expect, test } from "../support/app";
import {
  expectNoAxeViolations,
  expectNoHorizontalScroll,
} from "../support/axe";
import { fileRow, openView, reader } from "../support/ui";

const PLAN = "projects/project-plan.md";

const VIEWPORTS = [
  { width: 1280, height: 850 },
  { width: 1024, height: 768 },
  { width: 700, height: 800 },
  { width: 640, height: 425 },
];

/**
 * Every screen the merged UI has, at four window sizes: nothing runs off the
 * side, and axe-core finds no violation with its default rule set. A failure
 * here is a defect to fix, not a rule to switch off.
 */
for (const viewport of VIEWPORTS)
  test.describe(`At ${viewport.width}×${viewport.height}`, () => {
    test.use({ viewport, onboardingCompleted: false });

    test("every screen fits and passes axe", async ({ folio }) => {
      // Fresh desktop launches now enter onboarding. Inspect that real UI,
      // then take its explicit skip route before the core journeys.
      await expect(
        folio.getByRole("heading", { name: "Welcome to Folio" }),
      ).toBeVisible();
      await expectNoHorizontalScroll(folio);
      await expectNoAxeViolations(folio);
      await folio
        .getByRole("button", { name: "Skip setup", exact: true })
        .click();
      // Before a folder: the empty state and its illustration.
      await expectNoHorizontalScroll(folio);
      await expectNoAxeViolations(folio);

      await addFolder(folio);
      await expectNoHorizontalScroll(folio);
      await expectNoAxeViolations(folio);

      // Search results, with the located excerpts an indexed folder shows.
      await folio.getByRole("searchbox", { name: "Search files" }).fill("plan");
      await folio.getByRole("button", { name: "Index this folder" }).click();
      await expect(folio.locator(".result-evidence").first()).toBeVisible();
      await expectNoHorizontalScroll(folio);
      await expectNoAxeViolations(folio);
      await folio.getByRole("searchbox", { name: "Search files" }).fill("");

      // The reader, with a long path and non-ASCII text.
      await fileRow(folio, "notes/tala-sa-proyekto.md").click();
      await expect(reader(folio, "tala-sa-proyekto.md")).toBeVisible();
      await expectNoHorizontalScroll(folio);
      await expectNoAxeViolations(folio);
      await folio.keyboard.press("Escape");

      // Organize: suggestions, then the exact preview.
      await openView(folio, "Organize");
      await folio.getByRole("button", { name: "Analyze", exact: true }).click();
      await expect(
        folio.getByRole("heading", { name: "Exact duplicates" }),
      ).toBeVisible();
      await expectNoHorizontalScroll(folio);
      await expectNoAxeViolations(folio);
      await folio
        .getByRole("checkbox", { name: new RegExp(PLAN.replace("/", "\\/")) })
        .check();
      await folio.getByRole("button", { name: "Preview 1 change" }).click();
      await expect(
        folio.getByRole("heading", {
          name: "Exact preview: nothing has changed yet",
        }),
      ).toBeVisible();
      await expectNoHorizontalScroll(folio);
      await expectNoAxeViolations(folio);

      // Graph, Ask & Search and Model Lab.
      for (const view of ["Graph", "Ask & Search", "Activity", "Model Lab"]) {
        await openView(folio, view);
        if (view === "Model Lab")
          await expect(
            folio.getByRole("button", { name: /^Download / }).first(),
          ).toBeVisible();
        if (view === "Ask & Search")
          await expect(
            folio.getByRole("textbox", { name: "Your request", exact: true }),
          ).toBeVisible();
        await expectNoHorizontalScroll(folio);
        await expectNoAxeViolations(folio);
        if (view === "Graph") {
          await folio
            .getByRole("button", { name: "List", exact: true })
            .click();
          await expectNoHorizontalScroll(folio);
          await expectNoAxeViolations(folio);
        }
      }
    });
  });
