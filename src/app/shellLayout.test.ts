import { describe, expect, it } from "vitest";
import {
  LIST_MIN_WIDTH,
  READER_DEFAULT_WIDTH,
  READER_MIN_WIDTH,
  SIDEBAR_FULL_WIDTH,
  SIDEBAR_RAIL_WIDTH,
  clampReaderWidth,
  computeShellLayout,
} from "./shellLayout";

describe("clampReaderWidth", () => {
  it("keeps a reasonable width unchanged", () => {
    expect(clampReaderWidth(380, 1440)).toBe(380);
  });

  it("never goes below the minimum", () => {
    expect(clampReaderWidth(100, 1440)).toBe(READER_MIN_WIDTH);
    expect(clampReaderWidth(-50, 1440)).toBe(READER_MIN_WIDTH);
  });

  it("never exceeds 60% of the window", () => {
    expect(clampReaderWidth(2000, 1000)).toBe(600);
  });

  it("falls back to the default for a non-finite request", () => {
    expect(clampReaderWidth(NaN, 1440)).toBe(READER_DEFAULT_WIDTH);
    expect(clampReaderWidth(Infinity, 1440)).toBe(READER_DEFAULT_WIDTH);
  });

  it("follows the window down to 60%, never below the minimum", () => {
    // 600 * 0.6 = 360, still above the 320 floor.
    expect(clampReaderWidth(380, 600)).toBe(360);
    expect(clampReaderWidth(380, 500)).toBe(READER_MIN_WIDTH);
  });
});

describe("computeShellLayout without a reader", () => {
  it("shows full sidebar labels whenever there's room, even below 1180px", () => {
    // 1180 is the old fixed breakpoint; with no reader open there's still
    // plenty of room for labels (#67 item 5).
    expect(computeShellLayout(1180, false).sidebarMode).toBe("full");
    expect(computeShellLayout(900, false).sidebarMode).toBe("full");
  });

  it("collapses to the icon rail only once the main area would be squeezed", () => {
    expect(computeShellLayout(719, false).sidebarMode).toBe("rail");
    expect(computeShellLayout(720, false).sidebarMode).toBe("full");
  });

  it("reports no reader", () => {
    const layout = computeShellLayout(1440, false);
    expect(layout.readerMode).toBe("none");
    expect(layout.readerWidth).toBe(0);
  });
});

describe("computeShellLayout with a reader", () => {
  it("splits list and reader side by side when both minimums fit", () => {
    const layout = computeShellLayout(1440, true, READER_DEFAULT_WIDTH);
    expect(layout.readerMode).toBe("split");
    expect(layout.sidebarMode).toBe("full");
    expect(layout.readerWidth).toBe(READER_DEFAULT_WIDTH);
  });

  it("collapses the sidebar to the rail before giving up the split", () => {
    // Full sidebar leaves no room, but the rail does.
    const width =
      SIDEBAR_RAIL_WIDTH + LIST_MIN_WIDTH + READER_DEFAULT_WIDTH + 10;
    const layout = computeShellLayout(width, true, READER_DEFAULT_WIDTH);
    expect(layout.sidebarMode).toBe("rail");
    expect(layout.readerMode).toBe("split");
  });

  it("overlays the reader once even the rail sidebar can't make room", () => {
    const width = SIDEBAR_RAIL_WIDTH + LIST_MIN_WIDTH + READER_MIN_WIDTH - 10;
    const layout = computeShellLayout(width, true, READER_DEFAULT_WIDTH);
    expect(layout.readerMode).toBe("overlay");
  });

  it("whenever split, the list keeps its minimum width (the name is never crushed)", () => {
    for (const width of [600, 794, 900, 1024, 1180, 1280, 1440, 1920]) {
      const layout = computeShellLayout(width, true, READER_DEFAULT_WIDTH);
      if (layout.readerMode !== "split") continue;
      const listWidth = width - layout.sidebarWidth - layout.readerWidth;
      expect(listWidth).toBeGreaterThanOrEqual(LIST_MIN_WIDTH);
    }
  });

  it("is idempotent at the exact boundary width", () => {
    const boundary = SIDEBAR_FULL_WIDTH + LIST_MIN_WIDTH + READER_DEFAULT_WIDTH;
    expect(computeShellLayout(boundary, true).readerMode).toBe("split");
    expect(computeShellLayout(boundary, true).sidebarMode).toBe("full");
    expect(computeShellLayout(boundary - 1, true).sidebarMode).toBe("rail");
  });
});
