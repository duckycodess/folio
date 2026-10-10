import { describe, expect, it } from "vitest";
import * as online from "./onlineGeneration";

describe("online generation adapter in the browser preview", () => {
  const calls: [string, () => Promise<unknown>][] = [
    ["onlineGenerationStatus", () => online.onlineGenerationStatus()],
    ["saveOnlineKey", () => online.saveOnlineKey("not-a-real-key")],
    ["forgetOnlineKey", () => online.forgetOnlineKey()],
    [
      "setOnlineGeneration",
      () => online.setOnlineGeneration(true, "openai/gpt-oss-20b", true),
    ],
  ];

  it("is unavailable outside the desktop app", () => {
    expect(online.isAvailable()).toBe(false);
  });

  for (const [name, run] of calls) {
    it(`${name} rejects instead of pretending to be set up`, async () => {
      await expect(run()).rejects.toMatchObject({
        code: "modelNotInstalled",
        details: { component: "runtime", reason: "browserPreview" },
      });
    });
  }
});
