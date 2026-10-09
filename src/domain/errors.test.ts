import { describe, expect, it } from "vitest";
import { FolioError, isFolioErrorCode, toFolioError } from "./errors";
import { FOLIO_ERROR_CODES } from "./contracts";

describe("failures crossing the boundary", () => {
  it("keeps a code this build knows how to act on", () => {
    const failure = toFolioError({
      code: "targetChanged",
      message: "This file changed.",
      details: { path: "projects/project-plan.md" },
    });
    expect(failure.code).toBe("targetChanged");
    expect(failure.details?.path).toBe("projects/project-plan.md");
  });

  it("does not present an unknown code as an actionable one", () => {
    const failure = toFolioError({
      code: "somethingNewerNativeSent",
      message: "A newer native core reported something else.",
    });
    expect(failure.code).toBe("internal");
    expect(failure.message).toBe(
      "A newer native core reported something else.",
    );
    expect(failure.details?.reportedCode).toBe("somethingNewerNativeSent");
  });

  it("carries details as strings, exactly like the native map", () => {
    const failure = toFolioError({
      code: "planStateInvalid",
      message: "x",
      details: { operationIndex: 2, retried: false },
    });
    expect(failure.details).toEqual({ operationIndex: "2", retried: "false" });
    for (const value of Object.values(failure.toPayload().details ?? {})) {
      expect(typeof value).toBe("string");
    }
  });

  it("recognizes every frozen code and nothing else", () => {
    for (const code of FOLIO_ERROR_CODES)
      expect(isFolioErrorCode(code)).toBe(true);
    expect(isFolioErrorCode("nope")).toBe(false);
    expect(isFolioErrorCode(undefined)).toBe(false);
  });

  it("reports a cancelled request as cancelled, not as an internal fault", () => {
    const aborted = new Error("The operation was aborted.");
    aborted.name = "AbortError";
    expect(toFolioError(aborted).code).toBe("cancelled");
  });

  it("falls back to internal for anything that is not a failure payload", () => {
    expect(toFolioError("a bare string").code).toBe("internal");
    expect(toFolioError(new FolioError("providerBusy", "busy")).code).toBe(
      "providerBusy",
    );
  });
});
