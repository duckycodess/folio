import { describe, expect, it } from "vitest";
import {
  LIST_MIN_WIDTH,
  READER_DEFAULT_WIDTH,
  READER_GUTTER,
  READER_MIN_WIDTH,
  SIDEBAR_FULL_WIDTH,
  SIDEBAR_RAIL_WIDTH,
  clampReaderWidth,
  computeShellLayout,
  readerMaxWidth,
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
    expect(clampReaderWidth(4000, 2000)).toBe(1200);
  });

  it("never grows so wide that it has to overlay the list", () => {
    // At 1024px, 60% would be 614px, but only 504px fits beside the list
    // and the gutter right of the reader.
    expect(clampReaderWidth(4000, 1024)).toBe(
      1024 - SIDEBAR_RAIL_WIDTH - LIST_MIN_WIDTH - READER_GUTTER,
    );
    expect(readerMaxWidth(1024)).toBe(504);
    // From the first width where both minimums fit beside the rail.
    const fits =
      SIDEBAR_RAIL_WIDTH + LIST_MIN_WIDTH + READER_MIN_WIDTH + READER_GUTTER;
    for (const width of [fits, 900, 1024, 1100, 1210, 1280]) {
      const layout = computeShellLayout(width, true, readerMaxWidth(width));
      expect(layout.readerMode).toBe("split");
    }
  });

  it("narrows a width remembered in a larger window instead of overlaying", () => {
    const remembered = clampReaderWidth(4000, 1440);
    const layout = computeShellLayout(1024, true, remembered);
    expect(layout.readerMode).toBe("split");
    expect(layout.readerWidth).toBe(504);
  });

  it("falls back to the default for a non-finite request", () => {
    expect(clampReaderWidth(NaN, 1440)).toBe(READER_DEFAULT_WIDTH);
    expect(clampReaderWidth(Infinity, 1440)).toBe(READER_DEFAULT_WIDTH);
  });

  it("follows the window down to 60%, never below the minimum", () => {
    // 500 * 0.6 = 300, still above the 280 floor.
    expect(clampReaderWidth(380, 500)).toBe(300);
    expect(clampReaderWidth(380, 400)).toBe(READER_MIN_WIDTH);
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
    // 222 + 480: the mockup's own rail breakpoint is 700px.
    expect(computeShellLayout(701, false).sidebarMode).toBe("rail");
    expect(computeShellLayout(702, false).sidebarMode).toBe("full");
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
      SIDEBAR_RAIL_WIDTH +
      LIST_MIN_WIDTH +
      READER_DEFAULT_WIDTH +
      READER_GUTTER +
      10;
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
      const listWidth =
        width - layout.sidebarWidth - layout.readerWidth - READER_GUTTER;
      expect(listWidth).toBeGreaterThanOrEqual(LIST_MIN_WIDTH);
    }
  });

  it("is idempotent at the exact boundary width", () => {
    const boundary =
      SIDEBAR_FULL_WIDTH +
      LIST_MIN_WIDTH +
      READER_DEFAULT_WIDTH +
      READER_GUTTER;
    expect(computeShellLayout(boundary, true).readerMode).toBe("split");
    expect(computeShellLayout(boundary, true).sidebarMode).toBe("full");
    expect(computeShellLayout(boundary - 1, true).sidebarMode).toBe("rail");
  });
});
