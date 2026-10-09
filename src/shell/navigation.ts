export type ViewId =
  "home" | "files" | "organize" | "graph" | "assistant" | "modelLab";

export interface NavItem {
  id: ViewId;
  label: string;
}

/**
 * Search, Organize and Summarize stay reachable without the assistant:
 * search lives in every header, Summarize in the document panel.
 */
export const PRIMARY_NAV: NavItem[] = [
  { id: "home", label: "Home" },
  { id: "files", label: "Files" },
  { id: "organize", label: "Organize" },
  { id: "graph", label: "Graph" },
  { id: "assistant", label: "Ask & Act" },
];

export const SECONDARY_NAV: NavItem[] = [
  { id: "modelLab", label: "Model Lab" },
];

export function isApplePlatform(platform: string): boolean {
  return /mac|iphone|ipad|ipod/i.test(platform);
}

export function searchShortcutLabel(platform: string): string {
  return isApplePlatform(platform) ? "⌘K" : "Ctrl K";
}

interface ShortcutEvent {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/** ⌘K on macOS, Ctrl+K elsewhere; never both, so Ctrl+K on a Mac is left alone. */
export function isSearchShortcut(
  event: ShortcutEvent,
  platform: string,
): boolean {
  if (event.key.toLowerCase() !== "k" || event.altKey || event.shiftKey)
    return false;
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
