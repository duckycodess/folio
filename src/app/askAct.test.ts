import { describe, expect, it } from "vitest";
import type {
  ApplyReport,
  DocumentRecord,
  InterpretationResult,
  SearchResult,
} from "../domain/contracts";
import {
  addTurn,
  appliedPath,
  changeKept,
  describeApplied,
  describeProposal,
  inScope,
  MAX_TURNS,
  matchReason,
  methodLabel,
  planAsk,
  preparingLabel,
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

describe("preparingLabel", () => {
  it("says what a request is preparing, and how far it is", () => {
    expect(
      preparingLabel({
        workspaceId: "w",
        phase: "reading",
        processed: 120,
        total: 400,
      }),
    ).toBe("Reading your files: 120 of 400");
    expect(
      preparingLabel({
        workspaceId: "w",
        phase: "embedding",
        processed: 64,
        total: 900,
      }),
    ).toBe("Preparing search by meaning: 64 of 900 passages");
  });

  it("has nothing to say without work, and never overshoots the total", () => {
    expect(preparingLabel(null)).toBeUndefined();
    expect(
      preparingLabel({
        workspaceId: "w",
        phase: "reading",
        processed: 0,
        total: 0,
      }),
    ).toBeUndefined();
    expect(
      preparingLabel({
        workspaceId: "w",
        phase: "embedding",
        processed: 12,
        total: 10,
      }),
    ).toBe("Preparing search by meaning: 10 of 10 passages");
  });
});

describe("planAsk", () => {
  const doc = (relativePath: string): DocumentRecord => ({
    id: `w:${relativePath}`,
    workspaceId: "w",
    relativePath,
    name: relativePath.split("/").pop() ?? relativePath,
    title: relativePath,
    language: "en",
    mediaType: "text/markdown",
    sizeBytes: 1,
  });
  const plan = doc("projects/project-plan.md");
  const budget = doc("personal/budget-notes.md");
  const question: InterpretationResult = {
    status: "nonMutating",
    intent: "question",
  };

  it("answers a question that names no file from the whole folder", () => {
    expect(planAsk(question, "When is the deadline?", undefined, "")).toEqual({
      kind: "answer",
      documentId: undefined,
    });
  });

  it("scopes a question to the one file the request names", () => {
    expect(
      planAsk({ ...question, document: plan }, "Plan deadline?", undefined, ""),
    ).toEqual({ kind: "answer", documentId: plan.id });
  });

  it("lets a file the user picked win over one Folio resolved", () => {
    expect(
      planAsk({ ...question, document: plan }, "Deadline?", budget, ""),
    ).toEqual({ kind: "answer", documentId: budget.id });
    expect(planAsk(question, "What does it say?", budget, "")).toEqual({
      kind: "answer",
      documentId: budget.id,
    });
  });

  it("summarizes a chosen or named file directly, else finds the target", () => {
    const summary: InterpretationResult = {
      status: "nonMutating",
      intent: "summarize",
      targetQuery: "the plan",
    };
    expect(planAsk(summary, "Summarize the plan", budget, "")).toEqual({
      kind: "summarize",
      document: budget,
    });
    expect(
      planAsk({ ...summary, document: plan }, "Summarize", undefined, ""),
    ).toEqual({ kind: "summarize", document: plan });
    expect(planAsk(summary, "Summarize the plan", undefined, "")).toEqual({
      kind: "findSummaryTarget",
      query: "the plan",
    });
    expect(
      planAsk(
        { status: "nonMutating", intent: "search" },
        "Find the plan",
        undefined,
        "",
      ),
    ).toEqual({ kind: "results", query: "Find the plan" });
  });

  it("asks which file for a question only when the request names several", () => {
    const selection: InterpretationResult = {
      status: "needsFileSelection",
      pendingIntent: "{}",
      purpose: "question",
      candidates: [
        result("notes/a-notes.md", "keyword"),
        result("b-notes.md", "keyword"),
      ],
    };
    const step = planAsk(selection, "What do the notes say?", undefined, "");
    expect(step).toMatchObject({
      kind: "outcome",
      outcome: { type: "chooseFile", purpose: "question" },
    });
    // Everything it named is outside the chosen folder scope: still a
    // question about the folder, not an empty chooser.
    expect(
      planAsk(selection, "What do the notes say?", undefined, "elsewhere"),
    ).toEqual({ kind: "answer" });
  });

  it("does not ask again which file when the user already chose one", () => {
    const selection = (
      purpose: "question" | "summarize",
    ): InterpretationResult => ({
      status: "needsFileSelection",
      pendingIntent: "{}",
      purpose,
      candidates: [
        result("a-notes.md", "keyword"),
        result("b-notes.md", "keyword"),
      ],
    });
    expect(planAsk(selection("question"), "Notes?", budget, "")).toEqual({
      kind: "answer",
      documentId: budget.id,
    });
    expect(
      planAsk(selection("summarize"), "Summarize notes", budget, ""),
    ).toEqual({ kind: "summarize", document: budget });
  });

  it("keeps the purpose of a selection and treats an older core's as a change", () => {
    const base = {
      status: "needsFileSelection" as const,
      pendingIntent: "{}",
      candidates: [result("a.md", "keyword")],
    };
    expect(planAsk(base, "Edit it", undefined, "")).toMatchObject({
      outcome: { purpose: "change" },
    });
    expect(
      planAsk({ ...base, purpose: "summarize" }, "Summarize", undefined, ""),
    ).toMatchObject({ outcome: { purpose: "summarize" } });
  });

  it("refuses a change to a file other than the chosen one before any preview", () => {
    const proposal: InterpretationResult = {
      status: "proposal",
      requestLanguage: "en",
      proposal: {
        kind: "rename",
        documentId: plan.id,
        relativePath: plan.relativePath,
        observedContentHash: "sha256:x",
        destinationRelativePath: "projects/plan-final.md",
      },
    };
    expect(planAsk(proposal, "Rename it", budget, "").kind).toBe("outcome");
    expect(planAsk(proposal, "Rename it", budget, "")).toMatchObject({
      outcome: { type: "otherFile" },
    });
    expect(planAsk(proposal, "Rename it", plan, "")).toMatchObject({
      outcome: { type: "proposal" },
    });
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
    doc("career/Sample_Resume.pdf"),
    doc("career/cover-letter.md"),
    doc("notes/resume-tips.md"),
    doc("notes/budget.txt"),
  ];

  it("finds a file by the name written in the request, whatever else it says", () => {
    const { named, exact } = namedFiles(
      documents,
      "Sample_Resume.pdf fine files",
    );
    expect(named.map((each) => each.document.relativePath)).toEqual([
      "career/Sample_Resume.pdf",
    ]);
    expect(exact.map((each) => each.relativePath)).toEqual([
      "career/Sample_Resume.pdf",
    ]);
  });

  it("names a file without its extension or case, but only exact names are written out", () => {
    const { named, exact } = namedFiles(documents, "find sample resume");
    expect(named.map((each) => each.document.name)).toEqual([
      "Sample_Resume.pdf",
    ]);
    expect(exact).toEqual([]);
  });

  it("keeps a generic one-word name after the index's results", () => {
    const files = [doc("a/notes.md"), doc("b/notes.md"), doc("c/to-do.md")];
    const found = namedFiles(files, "find notes about the budget");
    expect(found.named).toEqual([]);
    expect(found.partial.map((each) => each.document.relativePath)).toEqual([
      "a/notes.md",
      "b/notes.md",
    ]);
    // Two short words aren't distinctive either.
    expect(namedFiles(files, "what do I need to do").named).toEqual([]);
    // Written out in full, a one-word name is still named.
    expect(
      namedFiles(files, "summarize a/notes.md").named.length,
    ).toBeGreaterThan(0);
  });

  it("matches names regardless of accents, like the index", () => {
    const files = [doc("reports/Niño_report.md")];
    expect(
      namedFiles(files, "find the nino report").named.map(
        (each) => each.document.name,
      ),
    ).toEqual(["Niño_report.md"]);
  });

  it("writes out a file name only as a whole name", () => {
    const files = [doc("a/notes.md"), doc("a/old-notes.md")];
    const { exact } = namedFiles(files, "summarize old-notes.md please");
    expect(exact.map((each) => each.name)).toEqual(["old-notes.md"]);
    // Sentence punctuation after a name still counts as writing it out.
    expect(
      namedFiles(files, "Summarize notes.md.").exact.map((each) => each.name),
    ).toEqual(["notes.md"]);
  });

  it("lists files sharing a word with the request after named ones", () => {
    const { named, partial } = namedFiles(documents, "my resume");
    expect(named).toEqual([]);
    expect(partial.map((each) => each.document.name)).toEqual([
      "Sample_Resume.pdf",
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

describe("an applied change", () => {
  const rename = {
    kind: "rename" as const,
    documentId: "w:VILAR_Resume.pdf",
    relativePath: "VILAR_Resume.pdf",
    observedContentHash: "sha256:a",
    destinationRelativePath: "VILAR.pdf",
  };
  function report(...statuses: ("succeeded" | "failed")[]): ApplyReport {
    return {
      batch: {
        planId: "p",
        planDigest: "sha256:d",
        startedAt: 1,
        finishedAt: 2,
        outcomes: statuses.map((status, operationIndex) => ({
          operationIndex,
          status,
        })),
        stopReason: statuses.includes("failed") ? "failed" : "completed",
      },
      historySettled: true,
      indexRefreshed: true,
    };
  }

  it("is kept only when every operation applied and none was undone", () => {
    expect(changeKept(report("succeeded"), null)).toBe(true);
    expect(changeKept(null, null)).toBe(false);
    expect(changeKept(report("succeeded", "failed"), null)).toBe(false);
    expect(
      changeKept(report("succeeded"), {
        planId: "p",
        undoneEntryIds: ["h1"],
        remainingEntryIds: [],
        indexRefreshed: true,
      }),
    ).toBe(false);
  });

  it("names what was done and where the file is now", () => {
    expect(describeApplied(rename)).toBe(
      "Renamed VILAR_Resume.pdf to VILAR.pdf",
    );
    expect(appliedPath(rename)).toBe("VILAR.pdf");
  });
});
