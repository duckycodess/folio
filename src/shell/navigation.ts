export type ViewId =
  | "home"
  | "organize"
  | "graph"
  | "assistant"
  | "activity"
  | "modelLab"
  | "settings";

export interface NavItem {
  id: ViewId;
  label: string;
}

/**
 * The brandkit mockup's navigation (docs/assets/Folio-Brandkit), with
 * Organize in the mockup's second slot: Search, Organize and Summarize stay
 * reachable without the assistant. Search lives on Home, Summarize in the
 * document panel.
 */
export const PRIMARY_NAV: NavItem[] = [
  { id: "home", label: "Home" },
  { id: "organize", label: "Organize" },
  { id: "graph", label: "Graph" },
  { id: "assistant", label: "Ask & Search" },
  { id: "activity", label: "Activity" },
];

/** At the foot of the sidebar; Model Lab is reached from Settings & style. */
export const SECONDARY_NAV: NavItem[] = [
  { id: "settings", label: "Settings & style" },
];

export function isApplePlatform(platform: string): boolean {
  return /mac|iphone|ipad|ipod/i.test(platform);
}

export function searchShortcutLabel(platform: string): string {
  return isApplePlatform(platform) ? "⌘K" : "Ctrl K";
}

interface ShortcutEvent {
  key: string;
  /** Physical key, used when the layout doesn't type Latin letters. */
  code?: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/**
 * ⌘K on macOS, Ctrl+K elsewhere; never both, so Ctrl+K on a Mac is left alone.
 * Latin layouts (including Dvorak) match the typed K; other layouts, such as
 * Cyrillic or Greek, match the key in the K position.
 */
export function isSearchShortcut(
  event: ShortcutEvent,
  platform: string,
): boolean {
  const isK = /^[a-z]$/i.test(event.key)
    ? event.key.toLowerCase() === "k"
    : event.code === "KeyK";
  if (!isK || event.altKey || event.shiftKey) return false;
  return isApplePlatform(platform)
    ? event.metaKey && !event.ctrlKey
    : event.ctrlKey && !event.metaKey;
}

export function currentPlatform(): string {
  if (typeof navigator === "undefined") return "";
  const withHints = navigator as Navigator & {
    userAgentData?: { platform?: string };
  };
  return withHints.userAgentData?.platform || navigator.platform || "";
}
