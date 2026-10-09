import { expect, type Locator, type Page } from "@playwright/test";
import { WORKSPACE_ID } from "./app";

/** One file's row button, found by the identity the native core gave it. */
export function fileRow(page: Page, relativePath: string): Locator {
  return page.locator(`[data-document-id="${WORKSPACE_ID}:${relativePath}"]`);
}

/** The ⋯ menu beside one file's row. */
export function rowMenu(page: Page, relativePath: string): Locator {
  return page
    .locator(`[data-row-id="${WORKSPACE_ID}:${relativePath}"]`)
    .locator(".row-menu-button");
}

export function sidebar(page: Page, label: string): Locator {
  return page.getByRole("button", { name: label, exact: true });
}

export function openView(page: Page, label: string): Promise<void> {
  return sidebar(page, label).click();
}

export function reader(page: Page, name: string): Locator {
  return page.getByRole("complementary", { name: `${name} details` });
}

/** Opens one file from the list and waits for its reader panel. */
export async function openFile(
  page: Page,
  relativePath: string,
): Promise<Locator> {
  await fileRow(page, relativePath).click();
  const name = relativePath.split("/").slice(-1)[0];
  const panel = reader(page, name);
  await expect(panel).toBeVisible();
  return panel;
}

/**
 * Presses Tab until the focused element matches `selector`, so a test can show
 * that a control is *reachable* from the keyboard rather than only that it
 * behaves once focus is put on it. Fails with what it did reach.
 */
export async function tabUntil(
  page: Page,
  selector: string,
  presses = 30,
): Promise<void> {
  for (let press = 0; press < presses; press += 1) {
    await page.keyboard.press("Tab");
    if (await page.locator(`${selector}:focus`).count()) return;
  }
  const reached = await page.evaluate(() => {
    const active = document.activeElement as HTMLElement | null;
    return active
      ? `${active.tagName.toLowerCase()} "${(active.textContent ?? "").trim().slice(0, 40)}"`
      : "nothing";
  });
  throw new Error(
    `${selector} was not reachable within ${presses} tabs; focus stopped at ${reached}`,
  );
}

/** The text of the page's polite live region. */
export function liveRegion(page: Page): Locator {
  return page.locator('[role="status"]');
}
