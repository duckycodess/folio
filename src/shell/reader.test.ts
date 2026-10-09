import { describe, expect, it } from "vitest";
import type { DocumentRecord, SearchResult } from "../domain/contracts";
import { listedSelection, readerDocument } from "./reader";

const doc = (id: string) => ({ id, name: `${id}.md` }) as DocumentRecord;
const listed = (...docs: DocumentRecord[]) =>
  docs.map((document) => ({ document }) as SearchResult);

describe("reader document", () => {
  const plan = doc("plan");
  const notes = doc("notes");

  it("opens nothing until a file is chosen", () => {
    expect(readerDocument("home", undefined, listed(plan, notes))).toBe(
      undefined,
    );
  });

  it("shows the chosen file while the list includes it", () => {
    expect(readerDocument("home", plan, listed(plan, notes))).toBe(plan);
  });

  it("hides a chosen file that the current search excludes", () => {
    expect(readerDocument("home", plan, listed(notes))).toBe(undefined);
  });

  it("shows files opened from Graph, which lists links rather than results", () => {
    expect(readerDocument("graph", plan, [])).toBe(plan);
  });

  it("keeps the rename form in Organize instead of opening the reader", () => {
    expect(readerDocument("organize", plan, listed(plan))).toBe(undefined);
    expect(readerDocument("assistant", plan, listed(plan))).toBe(undefined);
  });
});

describe("listed selection", () => {
  const plan = doc("plan");
  const notes = doc("notes");

  it("keeps the chosen file while the search includes it", () => {
    expect(listedSelection(plan, listed(plan, notes))).toBe(plan);
  });

  it("drops a chosen file the search leaves out, so Organize can't act on it", () => {
    expect(listedSelection(plan, listed(notes))).toBe(undefined);
    expect(listedSelection(undefined, listed(plan))).toBe(undefined);
  });
});
