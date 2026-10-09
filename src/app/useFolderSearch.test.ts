import { describe, expect, it } from "vitest";
import { indexAfterScan } from "./useFolderSearch";

describe("folder text search after a scan", () => {
  it("is searchable once the index holds any file", () => {
    expect(indexAfterScan(12)).toBe("ready");
  });

  it("searches only names when a stopped scan indexed nothing", () => {
    // The native core reports Stop as a summary marked `cancelled`, not an
    // error; with nothing indexed yet, text search must not be offered.
    expect(indexAfterScan(0)).toBe("missing");
  });
});
