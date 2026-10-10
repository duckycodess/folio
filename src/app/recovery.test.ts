import { describe, expect, it } from "vitest";
import { FOLIO_ERROR_CODES, PROVIDER_ERROR_CODES } from "../domain/contracts";
import { toFolioError } from "../domain/errors";
import { simulationFrom } from "../adapters/simulate";
import { RECOVERY, recoveryFor } from "./recovery";

const entries = FOLIO_ERROR_CODES.map(
  (code) => [code, RECOVERY[code]] as const,
);

const STAGES = ["refused", "duringApply", "partialUndo"] as const;

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
    for (const [code] of entries)
      for (const stage of STAGES) {
        const { title, message, action } = recoveryFor({ code }, stage);
        const text = `${title} ${message} ${action?.label ?? ""}`;
        expect(text, `${code} ${stage}`).not.toMatch(JARGON);
        // camelCase identifiers, which covers every multi-word code name
        expect(text, `${code} ${stage}`).not.toMatch(/[a-z][A-Z]/);
      }
  });

  it("says no file was changed when a change is refused before any write", () => {
    for (const [code, recovery] of entries)
      if (
        recovery.flow === "changes" &&
        !recovery.afterWrite &&
        code !== "planEmpty"
      )
        expect(recoveryFor({ code }, "refused").message, code).toContain(
          "No file was changed.",
        );
  });

  it("never says nothing changed after a write or a partial Undo", () => {
    for (const [code] of entries)
      for (const stage of ["duringApply", "partialUndo"] as const) {
        const { title, message } = recoveryFor({ code }, stage);
        expect(`${title} ${message}`, `${code} ${stage}`).not.toMatch(
          /No file was changed|didn't undo anything/,
        );
      }
    // The writer reports this only after the file changed, at any stage.
    const { title, message } = RECOVERY.historyRequired;
    expect(`${title} ${message}`).not.toMatch(/No file was changed/);
    expect(title).toMatch(/was changed/);
  });

  it("says earlier changes were kept when a batch stops partway", () => {
    const stopped = recoveryFor({ code: "targetChanged" }, "duringApply");
    expect(stopped.message).toContain(
      "Earlier changes in this batch were kept",
    );
    expect(
      recoveryFor({ code: "destinationExists" }, "duringApply").action?.kind,
    ).toBe("chooseAnotherName");
  });

  it("says a partial Undo kept what it reversed", () => {
    const partial = recoveryFor({ code: "undoConflict" }, "partialUndo");
    expect(partial.message).toMatch(/undid part of this change/);
    expect(partial.action?.label).toBe("Preview Undo again");
  });

  it("doesn't offer to retry a change whose plan is already spent", () => {
    expect(RECOVERY.historyRequired.action).toBeUndefined();
    for (const [code, recovery] of entries)
      if (recovery.flow === "changes")
        expect(
          recoveryFor({ code }, "duringApply").action?.kind,
          code,
        ).not.toBe("retry");
  });

  it("ignores the stage outside change-related errors", () => {
    expect(recoveryFor({ code: "modelNotInstalled" }, "duringApply")).toBe(
      RECOVERY.modelNotInstalled,
    );
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

describe("a busy local model", () => {
  it("names what is running and offers to stop it", () => {
    const recovery = recoveryFor({
      code: "providerBusy",
      details: { holder: "summary" },
    });
    expect(recovery.title).toBe("Folio is writing a summary");
    expect(recovery.action).toEqual({
      kind: "stopAndRetry",
      label: "Stop it and try again",
    });
  });

  it("names Organize's suggestions when they hold the model", () => {
    expect(
      recoveryFor({
        code: "providerBusy",
        details: { holder: "organizeSuggestions" },
      }).title,
    ).toBe("Folio is naming suggestions in Organize");
  });

  it("keeps the plain retry when the holder is unknown", () => {
    const recovery = recoveryFor({ code: "providerBusy" });
    expect(recovery.title).toBe("Folio is still working on another request");
    expect(recovery.action?.kind).toBe("retry");
  });
});
