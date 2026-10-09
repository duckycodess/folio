import { describe, expect, it } from "vitest";
import { discoverExplicitReferences, keywordSearch } from "./discovery";
import { validateRelationship } from "./relationships";
import { assertPassageMatches, sliceByUtf8Offsets } from "./offsets";
import { hashText } from "./hash";
import { documentIdFor, mediaTypeForPath } from "./identity";
import { loadFixtureDocuments } from "../adapters/workspace";
import { FIXTURE_WORKSPACE_ID, type DocumentRecord } from "./contracts";

async function doc(
  relativePath: string,
  content: string,
): Promise<DocumentRecord> {
  return {
    id: documentIdFor(FIXTURE_WORKSPACE_ID, relativePath),
    workspaceId: FIXTURE_WORKSPACE_ID,
    relativePath,
    name: relativePath.split("/").at(-1)!,
    title: relativePath,
    language: "mixed",
    mediaType: mediaTypeForPath(relativePath) ?? "text/plain",
    sizeBytes: new TextEncoder().encode(content).length,
    contentHash: await hashText(content),
    content,
  };
}

describe("keyword retrieval", () => {
  it("finds Filipino text and never presents it as semantic retrieval", async () => {
    const notes = await doc("notes.md", "Ang pagpupulong ay sa Biyernes.");
    const results = keywordSearch(
      [notes, await doc("other.md", "Grocery list")],
      "pagpupulong Biyernes",
    );
    expect(results).toHaveLength(1);
    expect(results[0].method).toBe("keyword");
    expect(results[0].spaceFingerprint).toBeUndefined();
    expect(results[0].passages[0].text).toContain("Biyernes");
  });

  it("binds every passage to the revision it was located in", async () => {
    const content = "Ang huling araw ay ika-20 ng Oktubre 📅.";
    const document = await doc("notes/paalala.md", content);
    const [result] = keywordSearch([document], "Oktubre");
    const passage = result.passages[0];
    expect(passage.documentContentHash).toBe(document.contentHash);
    expect(sliceByUtf8Offsets(content, passage.start, passage.end)).toBe(
      passage.text,
    );
    assertPassageMatches(passage, content, document.contentHash!);
  });

  it("produces no evidence for a document whose revision is unknown", async () => {
    const document = await doc("notes.md", "Ang pagpupulong ay sa Biyernes.");
    const withoutHash: DocumentRecord = { ...document };
    delete withoutHash.contentHash;
    const [result] = keywordSearch([withoutHash], "pagpupulong");
    expect(result.passages).toEqual([]);
  });

  it("does not retrieve across languages, which is why semantic search is still required", async () => {
    const filipino = await doc(
      "notes/tala.md",
      "Ang huling araw ng pagpasa ay ika-20 ng Oktubre.",
    );
    expect(keywordSearch([filipino], "submission deadline")).toHaveLength(0);
  });
});

describe("explicit references", () => {
  it("records the link, both revisions and a passage in the source", async () => {
    const documents = [
      await doc(
        "meetings/notes.md",
        "[Plan](../projects/plan.md) [Web](https://example.com) [Escape](../../outside.md)",
      ),
      await doc("projects/plan.md", "Due October 20"),
    ];
    const edges = discoverExplicitReferences(documents);
    expect(edges).toHaveLength(1);
    const edge = edges[0];
    expect(edge.type).toBe("explicitReference");
    expect(edge.targetId).toBe(documents[1].id);
    if (edge.type !== "explicitReference") return;
    expect(edge.link.resolvedRelativePath).toBe("projects/plan.md");
    expect(edge.evidence[0].text).toBe("[Plan](../projects/plan.md)");
    validateRelationship(edge);
  });

  it("ignores a link whose target revision is unknown", async () => {
    const source = await doc(
      "meetings/notes.md",
      "[Plan](../projects/plan.md)",
    );
    const target = await doc("projects/plan.md", "Due October 20");
    delete target.contentHash;
    expect(discoverExplicitReferences([source, target])).toEqual([]);
  });
});

describe("the synthetic corpus", () => {
  it("carries English, Filipino and Taglish documents with real revisions", async () => {
    const documents = await loadFixtureDocuments();
    expect(documents.length).toBeGreaterThanOrEqual(15);
    for (const document of documents) {
      expect(document.contentHash).toMatch(/^sha256:[0-9a-f]{64}$/);
      expect(document.id).toBe(
        `${FIXTURE_WORKSPACE_ID}:${document.relativePath}`,
      );
      expect(document.sizeBytes).toBe(
        new TextEncoder().encode(document.content!).length,
      );
    }
    expect(documents.map((item) => item.language)).toEqual(
      expect.arrayContaining(["en", "fil", "mixed"]),
    );
  });

  it("retrieves the Filipino and Taglish deadline documents by their own words", async () => {
    const documents = await loadFixtureDocuments();
    const filipino = keywordSearch(documents, "huling araw ng pagpasa");
    expect(filipino.map((item) => item.document.relativePath)).toContain(
      "notes/tala-sa-proyekto.md",
    );
    const taglish = keywordSearch(documents, "deadline ng Community Learning");
    expect(taglish.map((item) => item.document.relativePath)).toContain(
      "meetings/meeting-notes.md",
    );
  });

  it("connects the demo documents through their actual Markdown links", async () => {
    const documents = await loadFixtureDocuments();
    const edges = discoverExplicitReferences(documents);
    for (const edge of edges) validateRelationship(edge);
    const toPlan = edges.filter(
      (edge) =>
        edge.targetId ===
        documentIdFor(FIXTURE_WORKSPACE_ID, "projects/project-plan.md"),
    );
    expect(toPlan.map((edge) => edge.sourceId)).toEqual(
      expect.arrayContaining([
        documentIdFor(FIXTURE_WORKSPACE_ID, "meetings/meeting-notes.md"),
        documentIdFor(FIXTURE_WORKSPACE_ID, "notes/tala-sa-proyekto.md"),
      ]),
    );
  });
});
