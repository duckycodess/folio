import { describe, expect, it } from "vitest";
import { FOLIO_ERROR_CODES, PROVIDER_ERROR_CODES } from "../domain/contracts";
import { toFolioError } from "../domain/errors";
import { simulationFrom } from "../adapters/simulate";
import { RECOVERY, recoveryFor } from "./recovery";

const entries = FOLIO_ERROR_CODES.map(
  (code) => [code, RECOVERY[code]] as const,
);

// Words that belong to the code, not to the person using Folio.
const JARGON =
  /\b(track\s*t\d|docs\/|engine|adapter|native|fixture|payload|digest|plan|workspace|internal|null|undefined|error code)\b/i;

describe("recovery wording", () => {
  it("covers every error code with a title, a message and a flow", () => {
    for (const [code, recovery] of entries) {
      expect(recovery.title, code).toMatch(/\S/);
      expect(recovery.title, code).not.toMatch(/\.$/);
      expect(recovery.message, code).toMatch(/\.$/);
      expect(recovery.flow, code).toBeTruthy();
    }
  });

  it("uses plain language, without developer terms or code names", () => {
    for (const [code, { title, message, action }] of entries) {
      const text = `${title} ${message} ${action?.label ?? ""}`;
      expect(text, code).not.toMatch(JARGON);
      // camelCase identifiers, which covers every multi-word code name
      expect(text, code).not.toMatch(/[a-z][A-Z]/);
    }
  });

  it("says no file was changed for every refused change", () => {
    for (const [code, recovery] of entries) {
      if (recovery.flow === "changes" && code !== "planEmpty")
        expect(recovery.message, code).toContain("No file was changed.");
    }
  });

  it("tells the user their request is kept when the local model fails", () => {
    for (const code of PROVIDER_ERROR_CODES)
      expect(RECOVERY[code].message, code).toContain("Your request is kept.");
  });

  it("offers setup, not a retry, when no model is installed", () => {
    expect(RECOVERY.modelNotInstalled.action?.kind).toBe("openModelLab");
  });

  it("offers another name, never an overwrite, when the name is taken", () => {
    expect(RECOVERY.destinationExists.action?.kind).toBe("chooseAnotherName");
    expect(RECOVERY.destinationExists.message).toMatch(/never replaces/);
  });

  it("treats a code this build doesn't know as a general failure", () => {
    const unknown = toFolioError({ code: "diskOnFire", message: "?" });
    expect(recoveryFor(unknown)).toBe(RECOVERY.internal);
  });
});

describe("browser practice mode", () => {
  it("simulates a known code in the browser preview", () => {
    expect(simulationFrom("?simulate=undoConflict", false)).toBe(
      "undoConflict",
    );
  });

  it("is never active in the desktop app", () => {
    expect(simulationFrom("?simulate=undoConflict", true)).toBeUndefined();
  });

  it("ignores codes that don't exist", () => {
    expect(simulationFrom("?simulate=diskOnFire", false)).toBeUndefined();
    expect(simulationFrom("", false)).toBeUndefined();
  });
});
