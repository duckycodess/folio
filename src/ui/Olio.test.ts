import { describe, expect, it } from "vitest";
import { OLIO_POSES, olioSource, type OlioSize } from "./Olio";

const SIZES: OlioSize[] = [48, 96, 160];

describe("Olio artwork", () => {
  it("has every pose at every display size", () => {
    for (const pose of OLIO_POSES)
      for (const size of SIZES)
        expect(() => olioSource(pose, size)).not.toThrow();
  });

  it("offers a 2x source for every size", () => {
    expect(olioSource("default", 48).src).toMatch(/olio-default-96/);
    expect(olioSource("default", 96).srcSet).toMatch(/olio-default-192.* 2x$/);
    expect(olioSource("default", 160).srcSet).toMatch(/olio-default-320.* 2x$/);
  });
});
