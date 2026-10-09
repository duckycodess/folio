import { describe, expect, it } from "vitest";
import {
  connectionsFor,
  describeConnection,
  highlightRange,
  localDuplicates,
  mergeRelationships,
  passageState,
  type Connection,
} from "./connections";
import type {
  DocumentRecord,
  DuplicateGroup,
  IndexedDocument,
  Relationship,
} from "./contracts";
import { hashText } from "./hash";
import { passageFromUtf16Range } from "./offsets";
import { documentId } from "./test-support";

const NOTES = documentId("meetings/meeting-notes.md");
const PLAN = documentId("projects/project-plan.md");
const BUDGET = documentId("projects/budget.md");

async function link(
  sourceId: string,
  targetId: string,
  text: string,
  linkText: string,
): Promise<Relationship> {
  const start = text.indexOf(linkText);
  const sourceContentHash = await hashText(text);
  return {
    type: "explicitReference",
    provenance: "documentLink",
    sourceId,
    targetId,
    sourceContentHash,
    targetContentHash: await hashText(`target ${targetId}`),
    link: { rawTarget: "x.md", resolvedRelativePath: "x.md" },
    evidence: [
      passageFromUtf16Range({
        documentId: sourceId,
        documentContentHash: sourceContentHash,
        text,
        startIndex: start,
        endIndex: start + linkText.length,
      }),
    ],
  };
}

function duplicateGroup(ids: string[]): DuplicateGroup {
  return {
    contentHash: "b".repeat(64),
    sizeBytes: 10,
    documents: ids.map((id) => ({ id }) as IndexedDocument),
  };
}

describe("connections of one document", () => {
  it("lists every related document, however many there are", async () => {
    const text = "Tingnan ang [plano](../projects/project-plan.md).";
    const many = await Promise.all(
      Array.from({ length: 12 }, (_, index) =>
        link(NOTES, documentId(`notes/${index}.md`), text, "[plano]"),
      ),
    );
    expect(connectionsFor(NOTES, many)).toHaveLength(12);
  });

  it("folds links in both directions into one connection with all evidence", async () => {
    const out = await link(NOTES, PLAN, "See [plan](p.md).", "[plan]");
    const back = await link(PLAN, NOTES, "From [notes](n.md).", "[notes]");
    const [connection] = connectionsFor(NOTES, [out, back]);
    expect(connection.direction).toBe("mutual");
    expect(connection.evidence.map((p) => p.documentId)).toEqual([NOTES, PLAN]);
  });

  it("keeps the direction of a one-way link", async () => {
    const out = await link(NOTES, PLAN, "See [plan](p.md).", "[plan]");
    expect(connectionsFor(NOTES, [out])[0].direction).toBe("outgoing");
    expect(connectionsFor(PLAN, [out])[0].direction).toBe("incoming");
  });

  it("does not repeat a link found both in the index and in an opened file", async () => {
    const indexed = await link(NOTES, PLAN, "See [plan](p.md).", "[plan]");
    const local = await link(NOTES, PLAN, "See [plan](p.md).", "[plan]");
    const merged = mergeRelationships([indexed], [local]);
    expect(merged).toHaveLength(1);
    expect(connectionsFor(NOTES, merged)[0].evidence).toHaveLength(1);
  });

  it("lists exact duplicates first, with hash provenance and no excerpt", async () => {
    const out = await link(NOTES, PLAN, "See [plan](p.md).", "[plan]");
    const connections = connectionsFor(
      NOTES,
      [out],
      [duplicateGroup([NOTES, BUDGET]), duplicateGroup([PLAN, PLAN + "x"])],
    );
    expect(connections.map((c) => [c.kind, c.otherId])).toEqual([
      ["exactDuplicate", BUDGET],
      ["explicitReference", PLAN],
    ]);
    expect(connections[0].provenance).toBe("contentHash");
  });
});

describe("exact duplicates from hashes Folio already has", () => {
  const doc = (id: string, contentHash?: string) =>
    ({ id, contentHash, sizeBytes: 5 }) as DocumentRecord;

  it("groups only documents with identical hashes", () => {
    const groups = localDuplicates([
      doc(NOTES, "c".repeat(64)),
      doc(PLAN, "d".repeat(64)),
      doc(BUDGET, "c".repeat(64)),
      doc("unread"),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0].documents.map((d) => d.id)).toEqual([NOTES, BUDGET]);
  });
});

describe("connection labels", () => {
  const base = { otherId: PLAN, evidence: [] };

  it("never describes links or identical bytes as AI output", () => {
    const factual: Connection[] = [
      {
        ...base,
        kind: "explicitReference",
        direction: "outgoing",
        provenance: "documentLink",
      },
      {
        ...base,
        kind: "exactDuplicate",
        direction: "mutual",
        provenance: "contentHash",
      },
    ];
    for (const connection of factual) {
      const label = describeConnection(connection);
      expect(`${label.type} ${label.provenance}`).not.toMatch(
        /\b(AI|model|suggest)/i,
      );
    }
  });

  it("says when a model suggested a shared fact, and asks to check it", () => {
    const label = describeConnection({
      ...base,
      kind: "sharedFactCandidate",
      direction: "mutual",
      provenance: "model",
    });
    expect(label.provenance).toMatch(/AI model/);
    expect(label.provenance).toMatch(/check/);
  });
});

describe("passages in the reader", () => {
  const text = "Pulong: ang 📅 deadline ay [plano](p.md) — Oktubre 20.";

  async function documentWith(content: string) {
    return {
      content,
      contentHash: await hashText(content),
    } satisfies Pick<DocumentRecord, "content" | "contentHash">;
  }

  it("finds a passage after multi-byte characters", async () => {
    const relationship = await link(NOTES, PLAN, text, "[plano](p.md)");
    if (relationship.type !== "explicitReference") throw new Error();
    const [passage] = relationship.evidence;
    const document = await documentWith(text);
    const range = highlightRange(passage, document);
    expect(range && text.slice(...range)).toBe("[plano](p.md)");
  });

  it("refuses to highlight a passage from an older revision", async () => {
    const relationship = await link(NOTES, PLAN, text, "[plano](p.md)");
    if (relationship.type !== "explicitReference") throw new Error();
    const [passage] = relationship.evidence;
    const edited = await documentWith(`Bago: ${text}`);
    expect(passageState(passage, edited)).toBe("changed");
    expect(highlightRange(passage, edited)).toBeNull();
    expect(passageState(passage, { content: undefined })).toBe("unread");
  });
});
