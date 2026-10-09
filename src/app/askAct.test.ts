import { describe, expect, it } from "vitest";
import type { DocumentRecord, SearchResult } from "../domain/contracts";
import {
  addTurn,
  describeProposal,
  inScope,
  MAX_TURNS,
  matchReason,
  methodLabel,
  namedFiles,
  summaryTarget,
  targetsChosenFile,
  updateTurn,
  type AskTurn,
} from "./askAct";

function result(
  relativePath: string,
  method: SearchResult["method"],
  text = "",
): SearchResult {
  const document = {
    id: relativePath,
    relativePath,
    name: relativePath,
  } as DocumentRecord;
  return {
    document,
    score: 1,
    method,
    passages: text
      ? [
          {
            documentId: relativePath,
            documentContentHash: "h",
            offsetUnit: "utf8Byte",
            start: 0,
            end: text.length,
            text,
          },
        ]
      : [],
  };
}

describe("match labels", () => {
  it("calls only embedding results semantic", () => {
    expect(methodLabel("keyword")).toBe("Keyword match");
    expect(methodLabel("semantic")).toBe("Semantic match");
    expect(methodLabel("hybrid")).toBe("Keyword + semantic");
  });

  it("explains a match with the request's words found in the excerpt", () => {
    const keyword = result(
      "a.md",
      "keyword",
      "Interview methods for the study",
    );
    expect(matchReason(keyword, "notes about interview methods")).toBe(
      "Contains “interview”, “methods”",
    );
    expect(
      matchReason(result("b.md", "semantic", "Panayam"), "interview"),
    ).toBe("Close in meaning to your request");
    expect(
      matchReason(
        result("c.md", "hybrid", "interview guide"),
        "interview tips",
      ),
    ).toBe("Contains “interview”, and close in meaning");
  });
});

describe("scope", () => {
  const results = [
    result("research/a.md", "keyword"),
    result("research-old/b.md", "keyword"),
    result("top.md", "keyword"),
  ];

  it("keeps results inside the chosen folder, not a folder that shares its prefix", () => {
    expect(
      inScope(results, "research").map((r) => r.document.relativePath),
    ).toEqual(["research/a.md"]);
    expect(inScope(results, "")).toHaveLength(3);
  });
});

describe("summary requests", () => {
  it("summarizes one clear match and asks when there are several or none", () => {
    expect(summaryTarget([result("a.md", "keyword")])?.id).toBe("a.md");
    expect(
      summaryTarget([result("a.md", "keyword"), result("b.md", "keyword")]),
    ).toBeNull();
    expect(summaryTarget([])).toBeNull();
  });
});

describe("turns", () => {
  const turn = (id: number): AskTurn => ({
    id,
    request: `r${id}`,
    action: "find",
    status: "done",
  });

  it("keeps earlier replies, bounded", () => {
    let turns: AskTurn[] = [];
    for (let id = 1; id <= MAX_TURNS + 3; id++)
      turns = addTurn(turns, turn(id));
    expect(turns).toHaveLength(MAX_TURNS);
    expect(turns[0].id).toBe(4);
  });

  it("updates only the named turn", () => {
    const turns = updateTurn([turn(1), turn(2)], 2, { status: "cancelled" });
    expect(turns.map((t) => t.status)).toEqual(["done", "cancelled"]);
  });
});

describe("proposals", () => {
  it("describes a change Folio won't make from here", () => {
    expect(
      describeProposal({
        kind: "rename",
        documentId: "x",
        relativePath: "a.md",
        observedContentHash: "h",
        destinationRelativePath: "b.md",
      }),
    ).toBe("Rename a.md to b.md");
  });
});

describe("the file the user chose", () => {
  const edit = {
    kind: "edit" as const,
    documentId: "workspace:projects/plan.md",
    relativePath: "projects/plan.md",
    observedContentHash: "h",
    find: "October 20",
    replace: "October 23",
    targetEvidence: {
      documentId: "workspace:projects/plan.md",
      documentContentHash: "h",
      offsetUnit: "utf8Byte" as const,
      start: 0,
      end: 10,
      text: "October 20",
    },
  };

  it("accepts a change to the chosen file", () => {
    expect(targetsChosenFile(edit, "workspace:projects/plan.md")).toBe(true);
  });

  it("refuses a change to any other file, even a close match", () => {
    expect(targetsChosenFile(edit, "workspace:projects/plan-copy.md")).toBe(
      false,
    );
    expect(
      targetsChosenFile(
        { ...edit, kind: "rename", destinationRelativePath: "b.md" },
        "workspace:notes/plan.md",
      ),
    ).toBe(false);
  });

  it("lets a new file through, since it changes no existing one", () => {
    expect(
      targetsChosenFile(
        {
          kind: "create",
          destinationRelativePath: "notes/new.md",
          content: "",
        },
        "workspace:projects/plan.md",
      ),
    ).toBe(true);
  });
});

describe("namedFiles", () => {
  function doc(relativePath: string): DocumentRecord {
    return {
      id: relativePath,
      relativePath,
      name: relativePath.split("/").at(-1)!,
    } as DocumentRecord;
  }
  const documents = [
    doc("career/VILAR_Resume.pdf"),
    doc("career/cover-letter.md"),
    doc("notes/resume-tips.md"),
    doc("notes/budget.txt"),
  ];

  it("finds a file by the name written in the request, whatever else it says", () => {
    const { named, exact } = namedFiles(
      documents,
      "VILAR_Resume.pdf fine files",
    );
    expect(named.map((each) => each.document.relativePath)).toEqual([
      "career/VILAR_Resume.pdf",
    ]);
    expect(exact.map((each) => each.relativePath)).toEqual([
      "career/VILAR_Resume.pdf",
    ]);
  });

  it("names a file without its extension or case, but only exact names are written out", () => {
    const { named, exact } = namedFiles(documents, "find vilar resume");
    expect(named.map((each) => each.document.name)).toEqual([
      "VILAR_Resume.pdf",
    ]);
    expect(exact).toEqual([]);
  });

  it("lists files sharing a word with the request after named ones", () => {
    const { named, partial } = namedFiles(documents, "my resume");
    expect(named).toEqual([]);
    expect(partial.map((each) => each.document.name)).toEqual([
      "VILAR_Resume.pdf",
      "resume-tips.md",
    ]);
    expect(partial.every((each) => each.method === "keyword")).toBe(true);
  });

  it("ignores short shared words", () => {
    expect(namedFiles([doc("a/to-do.md")], "go to the store").partial).toEqual(
      [],
    );
  });
});

describe("summaryTarget with a file written out by name", () => {
  it("uses the one exact name even among several candidates", () => {
    const candidates = [result("a.md", "hybrid"), result("b.md", "hybrid")];
    expect(summaryTarget(candidates, [candidates[1].document])?.id).toBe(
      "b.md",
    );
    expect(summaryTarget(candidates, [])).toBeNull();
  });
});
