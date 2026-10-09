import { describe, expect, it } from "vitest";
import * as modelLab from "./modelLab";

describe("Model Lab adapter in the browser preview", () => {
  const calls: [string, () => Promise<unknown>][] = [
    ["labModels", () => modelLab.labModels()],
    [
      "runModelLab",
      () =>
        modelLab.runModelLab({
          embeddingModelId: "e",
          generationModelIds: ["g"],
        }),
    ],
    ["cancelModelLab", () => modelLab.cancelModelLab()],
    ["listLabRuns", () => modelLab.listLabRuns()],
    ["listLabResults", () => modelLab.listLabResults()],
    [
      "recordLabReview",
      () =>
        modelLab.recordLabReview({
          id: "r",
          outputSha256: "h",
          status: "correct",
          reviewer: "t",
        }),
    ],
    ["onLabProgress", async () => modelLab.onLabProgress(() => undefined)],
  ];

  it("is unavailable outside the desktop app", () => {
    expect(modelLab.isAvailable()).toBe(false);
  });

  for (const [name, run] of calls) {
    it(`${name} rejects instead of returning invented results`, async () => {
      await expect(run()).rejects.toMatchObject({
        code: "modelNotInstalled",
        details: { component: "runtime", reason: "browserPreview" },
      });
    });
  }
});
