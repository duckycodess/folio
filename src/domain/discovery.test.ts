import { expect, it } from "vitest";
import {
  discoverExplicitReferences,
  keywordSearch,
  sameEmbeddingSpace,
} from "./discovery";
import type { DocumentRecord, EmbeddingSpace } from "./contracts";

function doc(path: string, content: string): DocumentRecord {
  return {
    id: path,
    relativePath: path,
    name: path.split("/").at(-1)!,
    title: path,
    language: "mixed",
    sizeBytes: content.length,
    content,
  };
}

it("finds Filipino text without presenting keyword matches as semantic results", () => {
  const results = keywordSearch(
    [
      doc("notes.md", "Ang pagpupulong ay sa Biyernes."),
      doc("other.md", "Grocery list"),
    ],
    "pagpupulong Biyernes",
  );
  expect(results).toHaveLength(1);
  expect(results[0].method).toBe("keyword");
  expect(results[0].passages[0].text).toContain("Biyernes");
});

it("discovers cross-folder document links with evidence and ignores external/escaping links", () => {
  const documents = [
    doc(
      "meetings/notes.md",
      "[Plan](../projects/plan.md) [Web](https://example.com) [Escape](../../outside.md)",
    ),
    doc("projects/plan.md", "Due October 20"),
  ];
  const edges = discoverExplicitReferences(documents);
  expect(edges).toHaveLength(1);
  expect(edges[0].targetId).toBe("projects/plan.md");
  expect(edges[0].type).toBe("explicitReference");
  expect(edges[0].evidence[0].text).toBe("[Plan](../projects/plan.md)");
});

it("isolates embedding revisions and preprocessing even when dimensions match", () => {
  const space: EmbeddingSpace = {
    modelId: "e5",
    revision: "a",
    quantization: "q8",
    dimensions: 384,
    preprocessingFingerprint: "query-passage-v1",
  };
  expect(sameEmbeddingSpace(space, { ...space })).toBe(true);
  expect(sameEmbeddingSpace(space, { ...space, revision: "b" })).toBe(false);
  expect(
    sameEmbeddingSpace(space, {
      ...space,
      preprocessingFingerprint: "raw-text",
    }),
  ).toBe(false);
});
