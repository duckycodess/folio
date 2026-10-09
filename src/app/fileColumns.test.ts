import { describe, expect, it } from "vitest";
import {
  COLUMN_DROP_ORDER,
  fileColumnsTemplate,
  NAME_MAX_WIDTH,
  NAME_MIN_WIDTH,
  nameColumnWidth,
  visibleColumns,
} from "./fileColumns";

describe("visibleColumns", () => {
  it("shows every column when there's plenty of room", () => {
    expect(visibleColumns(1200)).toEqual([
      "location",
      "type",
      "modified",
      "size",
    ]);
  });

  it("drops columns one at a time, in priority order, as the width shrinks", () => {
    const seen: string[][] = [];
    let width = 1200;
    let last = visibleColumns(width);
    seen.push(last);
    while (last.length > 0 && width > 0) {
      width -= 20;
      const next = visibleColumns(width);
      if (next.length < last.length) {
        expect(last.length - next.length).toBe(1);
        seen.push(next);
        last = next;
      }
    }
    // Each step removes exactly the next column in the documented drop
    // order (size, then modified, then type, then location).
    const droppedInOrder = seen
      .slice(0, -1)
      .map((columns, index) =>
        columns.find((column) => !seen[index + 1].includes(column)),
      );
    expect(droppedInOrder).toEqual(COLUMN_DROP_ORDER);
  });

  it("never drops the name column (it isn't one of the four)", () => {
    expect(visibleColumns(0)).toEqual([]);
    // Name is implicit/always present; these four are the only droppable
    // ones, and at width 0 all four are gone, leaving just the name.
  });

  it("keeps at least the name's minimum width available once every column is dropped", () => {
    expect(visibleColumns(NAME_MIN_WIDTH + 32)).toEqual([]);
  });
});

describe("name column width", () => {
  it("drops a column sooner when the longest name needs more room", () => {
    // 160 + 4 columns (448) + 5 gaps (80) = 688 fits a short name only.
    expect(visibleColumns(688, NAME_MIN_WIDTH)).toHaveLength(4);
    expect(visibleColumns(688, 200)).toEqual(["location", "type", "modified"]);
  });

  it("follows the longest name, within its minimum and maximum", () => {
    expect(nameColumnWidth(90)).toBe(NAME_MIN_WIDTH);
    expect(nameColumnWidth(240)).toBe(240);
    expect(nameColumnWidth(900)).toBe(NAME_MAX_WIDTH);
    expect(nameColumnWidth(Number.NaN)).toBe(NAME_MIN_WIDTH);
  });
});

describe("fileColumnsTemplate", () => {
  it("always starts with a flexible name track", () => {
    expect(fileColumnsTemplate([])).toBe("minmax(0, 1fr)");
  });

  it("orders tracks as Location, Type, Modified, Size regardless of input order", () => {
    expect(fileColumnsTemplate(["size", "location"])).toBe(
      "minmax(0, 1fr) 160px 80px",
    );
  });
});
