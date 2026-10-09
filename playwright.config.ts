import { defineConfig, devices } from "@playwright/test";

const CI = Boolean(process.env.CI);

/**
 * Browser journeys for issue #9.
 *
 * One worker and one browser on purpose: these run beside a server on a machine
 * with little spare memory, and the point is a reliable signal in about two
 * minutes, not parallel throughput.
 */
export default defineConfig({
  testDir: "./e2e/specs",
  testMatch: /.*\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  forbidOnly: CI,
  retries: 0,
  timeout: 30_000,
  // The whole suite, server included, is meant to stay near two minutes.
  globalTimeout: 110_000,
  expect: { timeout: 7_000 },
  reporter: [["list"], ["html", { open: "never" }]],
  outputDir: "test-results",
  use: {
    baseURL: "http://127.0.0.1:1421",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "off",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1280, height: 850 },
      },
    },
  ],
  // The journeys run against the production build, so the same bundle
  // `npm run check:bundle` inspects is the one under test.
  webServer: {
    command: "npm run build && npm run preview:e2e",
    url: "http://127.0.0.1:1421",
    // Always this run's own server on its own port: never a dev server left
    // running on another branch.
    reuseExistingServer: false,
    timeout: 120_000,
    stdout: "ignore",
    stderr: "pipe",
  },
});
