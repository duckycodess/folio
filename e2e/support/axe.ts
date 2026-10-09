import { expect, type Page } from "@playwright/test";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const AXE_SOURCE = require.resolve("axe-core/axe.min.js");

interface AxeViolation {
  id: string;
  impact: string | null;
  help: string;
  nodes: { target: string[] }[];
}

declare global {
  interface Window {
    axe: {
      run(context: unknown): Promise<{ violations: AxeViolation[] }>;
    };
  }
}

/**
 * Every rule axe-core applies to this page by default. No rule is filtered out
 * and none is disabled: a rule that would fail is a defect to fix, not one to
 * hide behind a narrower rule set.
 */
export async function expectNoAxeViolations(page: Page): Promise<void> {
  await page.addScriptTag({ path: AXE_SOURCE });
  const violations = await page.evaluate(async () => {
    const result = await window.axe.run(document);
    return result.violations.map((violation) => ({
      id: violation.id,
      impact: violation.impact,
      help: violation.help,
      targets: violation.nodes.map((node) => node.target.join(" ")).slice(0, 4),
    }));
  });
  expect(violations, JSON.stringify(violations, null, 2)).toEqual([]);
}

/**
 * Nothing runs off the side of the window, and nothing the user can see has a
 * sideways scrollbar of its own: a panel that scrolls horizontally hides half
 * its content just as a page that does, and the page's own `scrollWidth` would
 * not show it.
 */
export async function expectNoHorizontalScroll(page: Page): Promise<void> {
  const overflow = await page.evaluate(() => {
    const root = document.documentElement;
    const scrollers: string[] = [];
    for (const element of Array.from(
      document.querySelectorAll<HTMLElement>("body *"),
    )) {
      const style = getComputedStyle(element);
      if (style.visibility === "hidden" || style.display === "none") continue;
      const box = element.getBoundingClientRect();
      if (box.width === 0 || box.height === 0) continue;
      const scrollable = /auto|scroll/.test(style.overflowX);
      if (!scrollable || element.scrollWidth <= element.clientWidth + 1)
        continue;
      scrollers.push(
        `${element.tagName.toLowerCase()}.${element.className} ${element.scrollWidth}>${element.clientWidth}`,
      );
    }
    return {
      scrollWidth: root.scrollWidth,
      clientWidth: root.clientWidth,
      scrollers,
    };
  });
  expect(
    overflow.scrollWidth,
    "the page itself scrolls sideways",
  ).toBeLessThanOrEqual(overflow.clientWidth);
  expect(overflow.scrollers, "these visible elements scroll sideways").toEqual(
    [],
  );
}
