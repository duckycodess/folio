import { expect, test } from "../support/app";
import { openView } from "../support/ui";

/**
 * The shell is exactly one window tall and only its panes scroll. Model Lab is
 * long enough that its visually-hidden announcer, absolutely positioned at the
 * end of the content, used to stretch the window and expose bare background
 * below the sidebar.
 */
for (const view of ["Home", "Model Lab", "Activity", "Organize"])
  test(`${view} never scrolls the window past the shell`, async ({ folio }) => {
    await folio.setViewportSize({ width: 1500, height: 880 });
    await openView(folio, view);
    const heights = await folio.evaluate(() => ({
      document: document.documentElement.scrollHeight,
      window: window.innerHeight,
    }));
    expect(heights.document).toBe(heights.window);
  });
