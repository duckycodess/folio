import { test as base, expect } from "@playwright/test";
import { RECOVERY } from "../../src/app/recovery";

/**
 * Browser practice mode. These run in a real browser with no fake native core
 * at all, which is the only way `?simulate=<code>` is reachable: it exists so
 * error wording can be checked in the preview and is never active in the
 * desktop app.
 */
base.describe("Browser practice mode", () => {
  base("is off until a code asks for it", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Practice mode", { exact: false })).toHaveCount(
      0,
    );
    // The preview says plainly that it has sample files, not a folder.
    await expect(page.locator(".topbar-badge")).toContainText("Sample files");
    await expect(
      page.getByText("Folder access works in the desktop app.", {
        exact: false,
      }),
    ).toBeVisible();
  });

  base(
    "simulates a folder failure where a folder is chosen",
    async ({ page }) => {
      await page.goto("/?simulate=workspaceUnavailable");
      await expect(page.locator(".notice-body").first()).toContainText(
        "Practice mode",
      );
      await page.getByRole("button", { name: "Add folder" }).first().click();
      await expect(page.locator(".notice-danger .notice-title")).toHaveText(
        RECOVERY.workspaceUnavailable.title,
      );
      await expect(
        page.getByRole("button", { name: "Add the folder again" }),
      ).toBeVisible();
    },
  );

  base("simulates a read failure where a file is opened", async ({ page }) => {
    await page.goto("/?simulate=documentTooLarge");
    await page.locator(".file-list .list-row").first().click();
    await expect(page.locator(".notice-warning .notice-title")).toHaveText(
      RECOVERY.documentTooLarge.title,
    );
  });

  base("ignores a code it does not know", async ({ page }) => {
    await page.goto("/?simulate=notARealCode");
    await expect(page.getByText("Practice mode", { exact: false })).toHaveCount(
      0,
    );
  });
});
