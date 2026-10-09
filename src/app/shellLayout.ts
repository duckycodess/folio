/**
 * How the shell lays out the sidebar, the main list and the reader, as a pure
 * function of the window's measured width. No fixed breakpoint decides this
 * on its own: a wide window with the reader closed keeps full sidebar labels
 * even below 1180px, and a narrow one with the reader open collapses sooner
 * (#67).
 */

/** docs/design.md's "Layout" table. */
export const SIDEBAR_FULL_WIDTH = 240;
export const SIDEBAR_RAIL_WIDTH = 64;
export const MAIN_MIN_WIDTH = 480;

/** The list keeps at least this much room beside the reader. */
export const LIST_MIN_WIDTH = 420;
/** The reader itself never gets narrower or (relatively) wider than this. */
export const READER_MIN_WIDTH = 320;
export const READER_MAX_RATIO = 0.6;
export const READER_DEFAULT_WIDTH = 380;

export type SidebarMode = "full" | "rail";
export type ReaderMode = "none" | "split" | "overlay";

export interface ShellLayout {
  sidebarMode: SidebarMode;
  sidebarWidth: number;
  readerMode: ReaderMode;
  /** 0 when `readerMode` is "none". */
  readerWidth: number;
}

/** Keeps a requested reader width inside its allowed range for this window. */
export function clampReaderWidth(
  requestedWidth: number,
  windowWidth: number,
): number {
  const max = Math.max(
    READER_MIN_WIDTH,
    Math.floor(windowWidth * READER_MAX_RATIO),
  );
  if (!Number.isFinite(requestedWidth)) return READER_DEFAULT_WIDTH;
  return Math.min(max, Math.max(READER_MIN_WIDTH, requestedWidth));
}

function sidebarModeWithoutReader(windowWidth: number): SidebarMode {
  return windowWidth - MAIN_MIN_WIDTH >= SIDEBAR_FULL_WIDTH ? "full" : "rail";
}

/**
 * The sidebar, list and reader follow the space actually available, not a
 * single window breakpoint: closing the reader can bring sidebar labels back
 * at a width that collapses them while it's open, and opening a wide reader
 * can collapse the sidebar at a width that keeps it full when the reader is
 * narrower.
 */
export function computeShellLayout(
  windowWidth: number,
  hasReader: boolean,
  requestedReaderWidth: number = READER_DEFAULT_WIDTH,
): ShellLayout {
  if (!hasReader) {
    const sidebarMode = sidebarModeWithoutReader(windowWidth);
    return {
      sidebarMode,
      sidebarWidth:
        sidebarMode === "full" ? SIDEBAR_FULL_WIDTH : SIDEBAR_RAIL_WIDTH,
      readerMode: "none",
      readerWidth: 0,
    };
  }

  const readerWidth = clampReaderWidth(requestedReaderWidth, windowWidth);
  const candidates: [SidebarMode, number][] = [
    ["full", SIDEBAR_FULL_WIDTH],
    ["rail", SIDEBAR_RAIL_WIDTH],
  ];
  for (const [sidebarMode, sidebarWidth] of candidates) {
    if (windowWidth - sidebarWidth - LIST_MIN_WIDTH - readerWidth >= 0) {
      return { sidebarMode, sidebarWidth, readerMode: "split", readerWidth };
    }
  }
  // Neither sidebar mode leaves room for the list beside the reader: the
  // reader overlays the list instead, which stays in place behind it.
  const sidebarMode = sidebarModeWithoutReader(windowWidth);
  return {
    sidebarMode,
    sidebarWidth:
      sidebarMode === "full" ? SIDEBAR_FULL_WIDTH : SIDEBAR_RAIL_WIDTH,
    readerMode: "overlay",
    readerWidth,
  };
}

const STORAGE_KEY = "folio.reader.width";

// Storage can be unavailable or throw (private windows, blocked site data);
// the width then simply isn't remembered.
export function loadReaderWidth(): number {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    const parsed = stored === null ? NaN : Number(stored);
    return Number.isFinite(parsed) ? parsed : READER_DEFAULT_WIDTH;
  } catch {
    return READER_DEFAULT_WIDTH;
  }
}

export function saveReaderWidth(width: number) {
  try {
    window.localStorage.setItem(STORAGE_KEY, String(Math.round(width)));
  } catch {
    // Not remembered; the current window still resizes.
  }
}
