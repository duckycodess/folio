import { test as base, expect, type Page } from "@playwright/test";
import { fixtureCorpus, fixtureModelMetadata } from "../fake/corpus";
import { installFakeNativeCore } from "../fake/nativeCore";
import type { FolioErrorCode } from "../../src/domain/contracts";
import type {
  FakeControl,
  FakeNativeOptions,
  FakeSkippedEntry,
  FakeWriterBehaviour,
} from "../fake/types";

/** The fake workspace's identity and root, as a real authorized folder's would be. */
export const WORKSPACE_ID = "e2e-workspace";
export const ROOT_PATH = "/Users/folio/Documents/Community Learning Project";

export const DEFAULT_OPTIONS: Omit<FakeNativeOptions, "files"> = {
  workspaceId: WORKSPACE_ID,
  rootPath: ROOT_PATH,
  authorizedAt: Date.UTC(2026, 9, 10, 8, 0),
  preIndexed: false,
  scanStepMs: 6,
  planLifetimeMs: 5 * 60 * 1000,
  skipped: [],
  dismissFolderPicker: false,
};

declare global {
  interface Window {
    __folioFake: FakeControl;
  }
}

/**
 * Installs the fake native core before any application module runs, so
 * `isTauri()` — read once at import time — sees the desktop app.
 */
export async function installFake(
  page: Page,
  overrides: Partial<FakeNativeOptions> = {},
  onboardingCompleted = true,
): Promise<void> {
  const options: FakeNativeOptions = {
    ...DEFAULT_OPTIONS,
    files: fixtureCorpus(),
    ...fixtureModelMetadata(),
    ...overrides,
  };
  await page.addInitScript(installFakeNativeCore, options);
  if (onboardingCompleted)
    await page.addInitScript(() => {
      window.localStorage.setItem("folio.onboarding.completed", "true");
    });
}

export const test = base.extend<{ folio: Page; onboardingCompleted: boolean }>({
  onboardingCompleted: [true, { option: true }],
  folio: async ({ page, onboardingCompleted }, use) => {
    await installFake(page, {}, onboardingCompleted);
    await page.goto("/");
    await use(page);
  },
});

export { expect };

/** Adds the workspace folder through the UI, exactly as a person would. */
export async function addFolder(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Add folder" }).first().click();
  await expect(page.locator(".topbar-badge")).toContainText(
    "Community Learning Project",
  );
}

/** Indexes the folder from Organize's Analyze step and waits for its suggestions. */
export async function analyzeFolder(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Organize", exact: true }).click();
  await page.getByRole("button", { name: "Analyze", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Exact duplicates" }),
  ).toBeVisible();
}

export function fake(page: Page) {
  return {
    read: (relativePath: string) =>
      page.evaluate((path) => window.__folioFake.readFile(path), relativePath),
    list: () => page.evaluate(() => window.__folioFake.listFiles()),
    calls: () => page.evaluate(() => window.__folioFake.calls()),
    failNext: (command: string, code: FolioErrorCode, message?: string) =>
      page.evaluate(
        (injected) =>
          window.__folioFake.failNext(injected.command, {
            code: injected.code,
            ...(injected.message ? { message: injected.message } : {}),
          }),
        { command, code, message },
      ),
    clearFailures: () =>
      page.evaluate(() => window.__folioFake.clearFailures()),
    setWriter: (behaviour: FakeWriterBehaviour) =>
      page.evaluate(
        (settings) => window.__folioFake.setWriter(settings),
        behaviour,
      ),
    externalEdit: (relativePath: string, content: string) =>
      page.evaluate(
        ([path, text]) => window.__folioFake.externalEdit(path, text),
        [relativePath, content] as const,
      ),
    setSkipped: (entries: FakeSkippedEntry[]) =>
      page.evaluate((list) => window.__folioFake.setSkipped(list), entries),
    setScanStepMs: (ms: number) =>
      page.evaluate((value) => window.__folioFake.setScanStepMs(value), ms),
    expirePlans: () => page.evaluate(() => window.__folioFake.expirePlans()),
    lastScan: () => page.evaluate(() => window.__folioFake.lastScan()),
  };
}
