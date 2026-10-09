import { describe, expect, it } from "vitest";
import { applyTheme, nextTheme, parseTheme } from "./theme";

describe("theme preference", () => {
  it("cycles System, Light, Dark and back", () => {
    expect(nextTheme("system")).toBe("light");
    expect(nextTheme("light")).toBe("dark");
    expect(nextTheme("dark")).toBe("system");
  });

  it("falls back to following the OS for missing or unknown values", () => {
    expect(parseTheme(null)).toBe("system");
    expect(parseTheme("sepia")).toBe("system");
    expect(parseTheme("dark")).toBe("dark");
  });

  it("removes the override for System so the OS setting applies", () => {
    const root = { dataset: {} as DOMStringMap } as HTMLElement;
    applyTheme("dark", root);
    expect(root.dataset.theme).toBe("dark");
    applyTheme("system", root);
    expect(root.dataset.theme).toBeUndefined();
  });
});
