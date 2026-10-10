import { describe, expect, it } from "vitest";
import type { DocumentRecord } from "./contracts";
import { keywordSearch } from "./discovery";
import { highlightSegments } from "./searchEvidence";
import { parseSearchQuery, scoreFile } from "./searchQuery";

// Mirrors `src-tauri/src/search_query.rs`'s tests: the two must agree.
function matches(query: string, relativePath: string, text = ""): boolean {
  const parsed = parseSearchQuery(query);
  if (!parsed) throw new Error(`${query} uses no operator`);
  const name = relativePath.split("/").at(-1)!;
  return scoreFile(parsed, { name, title: name, relativePath, text }) !== null;
}

describe("search operators", () => {
  it("leaves plain queries to the plain search", () => {
    expect(parseSearchQuery("budget notes")).toBeNull();
    expect(parseSearchQuery("budget or notes")).toBeNull();
    expect(parseSearchQuery("pre-school")).toBeNull();
  });

  it("matches quoted text only as that exact token", () => {
    expect(matches('"1_b"', "a.md", "Section 1_b covers fees.")).toBe(true);
    expect(matches('"1_b"', "a.md", "SECTION 1_B")).toBe(true);
    for (const other of [
      "Section 1_c",
      "Section 1 b",
      "Section 2_b",
      "Section 11_b",
      "Section 1_bc",
      "x_1_b",
    ])
      expect(matches('"1_b"', "a.md", other), other).toBe(false);
  });

  it("matches quoted text in a file name", () => {
    expect(matches('"1_b"', "forms/1_b.pdf")).toBe(true);
    expect(matches('"1_b"', "forms/1_c.pdf")).toBe(false);
  });

  it("matches a phrase across line breaks and accents", () => {
    expect(matches('"project plan"', "a.md", "the Project\n  plan")).toBe(true);
    expect(matches('"nino"', "a.md", "si Niño")).toBe(true);
    expect(matches('"project plan"', "a.md", "the plan for the project")).toBe(
      false,
    );
  });

  it("leaves out excluded words", () => {
    expect(matches("budget -draft", "a.md", "final budget")).toBe(true);
    expect(matches("budget -draft", "a.md", "draft budget")).toBe(false);
    expect(matches('budget -"old plan"', "a.md", "budget, old plan")).toBe(
      false,
    );
    expect(matches("budget -draft", "draft.md", "budget")).toBe(false);
  });

  it("accepts either side of an uppercase OR and requires the rest", () => {
    expect(matches('"budget" OR gastos', "a.md", "mga gastos")).toBe(true);
    expect(matches('"budget" OR gastos', "a.md", "nothing")).toBe(false);
    expect(matches('"budget" AND gastos', "a.md", "the budget")).toBe(false);
    expect(matches('"budget" AND gastos', "a.md", "budget at gastos")).toBe(
      true,
    );
  });

  it("filters by title, type and folder", () => {
    expect(matches("intitle:resume", "career/VILAR_Resume.pdf")).toBe(true);
    expect(matches("intitle:resume", "career/cover.pdf", "my resume")).toBe(
      false,
    );
    expect(matches("filetype:pdf", "a/b.pdf")).toBe(true);
    expect(matches("ext:.pdf", "a/b.md")).toBe(false);
    expect(matches("in:projects", "work/projects/2024/a.md")).toBe(true);
    expect(matches("folder:work/projects", "work/projects/2024/a.md")).toBe(
      true,
    );
    expect(matches("in:projects", "work/project/a.md")).toBe(false);
    expect(matches("in:projects", "projects.md")).toBe(false);
  });

  it("treats unknown fields as plain text", () => {
    expect(parseSearchQuery('"x" note:today')?.clauses).toEqual([
      ["x"],
      ["note:today"],
    ]);
  });
});

describe("name search with operators", () => {
  const documents = ["forms/1_b.pdf", "forms/1_c.pdf", "forms/1 b.pdf"].map(
    (relativePath) =>
      ({
        id: relativePath,
        relativePath,
        name: relativePath.split("/").at(-1)!,
        title: relativePath.split("/").at(-1)!,
      }) as DocumentRecord,
  );

  it("finds only the exact name", () => {
    expect(
      keywordSearch(documents, '"1_b"').map((each) => each.document.name),
    ).toEqual(["1_b.pdf"]);
  });
});

describe("highlighting with operators", () => {
  it("marks only the exact token", () => {
    expect(
      highlightSegments("1 b, 1_c and 1_b", '"1_b"')
        .filter((segment) => segment.hit)
        .map((segment) => segment.text),
    ).toEqual(["1_b"]);
  });
});
