export type ThemePreference = "system" | "light" | "dark";

const ORDER: ThemePreference[] = ["system", "light", "dark"];
const STORAGE_KEY = "folio.theme";

export const THEME_LABELS: Record<ThemePreference, string> = {
  system: "System",
  light: "Light",
  dark: "Dark",
};

/** The switch cycles System → Light → Dark → System. */
export function nextTheme(current: ThemePreference): ThemePreference {
  return ORDER[(ORDER.indexOf(current) + 1) % ORDER.length];
}

/** Anything unknown or missing falls back to following the OS. */
export function parseTheme(stored: string | null): ThemePreference {
  return ORDER.includes(stored as ThemePreference)
    ? (stored as ThemePreference)
    : "system";
}

// Storage can be unavailable or throw (private windows, blocked site data);
// the theme then simply isn't remembered.
export function loadTheme(): ThemePreference {
  try {
    return parseTheme(window.localStorage.getItem(STORAGE_KEY));
  } catch {
    return "system";
  }
}

export function saveTheme(theme: ThemePreference) {
  try {
    if (theme === "system") window.localStorage.removeItem(STORAGE_KEY);
    else window.localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Not remembered; the current window still switches.
  }
}

/** `system` removes the override so tokens.css follows the OS setting. */
export function applyTheme(theme: ThemePreference, root: HTMLElement) {
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
}
