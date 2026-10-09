import { describe, expect, it } from "vitest";
import providerError from "../../fixtures/contracts/provider-error.json";
import modelDescriptor from "../../fixtures/contracts/model-descriptor.json";
import groundedAnswer from "../../fixtures/contracts/grounded-answer.json";
import interpretationResult from "../../fixtures/contracts/interpretation-result.json";
import type {
  GroundedAnswer,
  InterpretationResult,
  ModelDescriptor,
  NativeProviderError,
} from "./contracts";

function hasOnlyCamelCaseKeys(value: unknown): boolean {
  if (Array.isArray(value)) return value.every(hasOnlyCamelCaseKeys);
  if (!value || typeof value !== "object") return true;
  return Object.entries(value).every(([key, child]) => {
    expect(key).not.toMatch(/_/);
    return hasOnlyCamelCaseKeys(child);
  });
}

describe("native contract goldens", () => {
  it("keeps provider and model keys typed and camelCase", () => {
    const error = providerError as NativeProviderError;
    const descriptor = modelDescriptor as ModelDescriptor;
    expect(error.code).toBe("modelNotInstalled");
    expect(descriptor.files[0].sha256).toHaveLength(64);
    expect(hasOnlyCamelCaseKeys(error)).toBe(true);
    expect(hasOnlyCamelCaseKeys(descriptor)).toBe(true);
  });

  it("keeps grounded citations and interpretation statuses typed", () => {
    const answer = groundedAnswer as GroundedAnswer;
    const result = interpretationResult as InterpretationResult;
    expect(answer.kind).toBe("fileSummary");
    expect(answer.sources[0].documentId).toBe("projects/project-plan.md");
    expect(result.status).toBe("needsClarification");
    expect(hasOnlyCamelCaseKeys(answer)).toBe(true);
    expect(hasOnlyCamelCaseKeys(result)).toBe(true);
  });
});
