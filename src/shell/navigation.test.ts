import { describe, expect, it } from "vitest";
import {
  isSearchShortcut,
  PRIMARY_NAV,
  searchShortcutLabel,
} from "./navigation";

const key = (overrides: Partial<Parameters<typeof isSearchShortcut>[0]>) => ({
  key: "k",
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...overrides,
});

describe("search shortcut", () => {
  it("shows the platform's modifier", () => {
    expect(searchShortcutLabel("MacIntel")).toBe("⌘K");
    expect(searchShortcutLabel("macOS")).toBe("⌘K");
    expect(searchShortcutLabel("Win32")).toBe("Ctrl K");
    expect(searchShortcutLabel("Windows")).toBe("Ctrl K");
  });

  it("uses Command on macOS and leaves Ctrl+K to the text field", () => {
    expect(isSearchShortcut(key({ metaKey: true }), "MacIntel")).toBe(true);
    expect(isSearchShortcut(key({ ctrlKey: true }), "MacIntel")).toBe(false);
  });

  it("uses Ctrl on Windows and ignores the Windows key", () => {
    expect(isSearchShortcut(key({ ctrlKey: true }), "Win32")).toBe(true);
    expect(isSearchShortcut(key({ metaKey: true }), "Win32")).toBe(false);
  });

  it("ignores other keys and extra modifiers", () => {
    expect(isSearchShortcut(key({ key: "j", ctrlKey: true }), "Win32")).toBe(
      false,
    );
    expect(
      isSearchShortcut(key({ ctrlKey: true, shiftKey: true }), "Win32"),
    ).toBe(false);
    expect(isSearchShortcut(key({ key: "K", ctrlKey: true }), "Win32")).toBe(
      true,
    );
  });

  it("works on keyboard layouts that don't type Latin letters", () => {
    expect(
      isSearchShortcut(key({ key: "л", code: "KeyK", ctrlKey: true }), "Win32"),
    ).toBe(true);
    expect(
      isSearchShortcut(key({ key: "κ", code: "KeyK", metaKey: true }), "macOS"),
    ).toBe(true);
    expect(
      isSearchShortcut(key({ key: "о", code: "KeyJ", ctrlKey: true }), "Win32"),
    ).toBe(false);
  });

  it("follows the typed letter on Latin layouts such as Dvorak", () => {
    // Dvorak types T in the QWERTY K position and K in the V position.
    expect(
      isSearchShortcut(key({ key: "t", code: "KeyK", ctrlKey: true }), "Win32"),
    ).toBe(false);
    expect(
      isSearchShortcut(key({ key: "k", code: "KeyV", ctrlKey: true }), "Win32"),
    ).toBe(true);
  });
});

describe("navigation", () => {
  it("keeps Organize as its own destination beside the assistant", () => {
    const labels = PRIMARY_NAV.map((item) => item.label);
    expect(labels).toContain("Organize");
    expect(labels).toContain("Ask & Act");
    expect(labels.indexOf("Organize")).toBeLessThan(
      labels.indexOf("Ask & Act"),
    );
  });
});
