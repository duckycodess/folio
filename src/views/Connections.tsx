import { Quote } from "lucide-react";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import { describeConnection, type Connection } from "../domain/connections";
import type { DocumentRecord, SourcePassage } from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { EmptyState } from "../ui/EmptyState";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { Notice } from "../ui/Notice";

const EXCERPT_LENGTH = 160;

function excerpt(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > EXCERPT_LENGTH
    ? `${flat.slice(0, EXCERPT_LENGTH - 1)}…`
    : flat;
}

/** The folder a file lives in on disk, or in the bundled samples. */
export function originalLocation(
  document: DocumentRecord,
  workspace: WorkspaceState,
): string {
  const folder = document.relativePath.split("/").slice(0, -1).join("/");
  const root = workspace.workspace?.rootPath ?? "Sample files";
  return folder ? `${root}/${folder}` : root;
}

/** Evidence excerpts; each opens its own document at the passage. */
export function ConnectionEvidence({
  evidence,
  byId,
  onOpen,
}: {
  evidence: SourcePassage[];
  byId: Map<string, DocumentRecord>;
  onOpen: (passage: SourcePassage) => void;
}) {
  if (!evidence.length) return null;
  return (
    <ul className="evidence-list">
      {evidence.map((passage) => {
        const where = byId.get(passage.documentId)?.name ?? "another file";
        return (
          <li key={`${passage.documentId}-${passage.start}-${passage.end}`}>
            <button
              type="button"
              className="evidence-item"
              onClick={() => onOpen(passage)}
              aria-label={`Show passage in ${where}: ${excerpt(passage.text)}`}
            >
              <Quote size={14} aria-hidden="true" />
              <span className="evidence-text">{excerpt(passage.text)}</span>
              <span className="evidence-where">
                in {where}
                {passage.page !== undefined && `, page ${passage.page}`}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

/** Says which connections Folio can see, so a short list isn't misleading. */
export function CoverageNote({ relations }: { relations: RelationshipsState }) {
  switch (relations.coverage) {
    case "samples":
      return (
        <p className="muted">
          Sample files: Folio shows the links written inside them.
        </p>
      );
    case "notIndexed":
      return (
        <p className="muted">
          This folder hasn't been indexed yet, so only links in files you've
          opened are shown.
        </p>
      );
    case "loading":
      return <p className="muted">Checking the folder index…</p>;
    case "failed":
      return (
        <Notice tone="warning">
          Folio couldn't read this folder's index, so only links in files you've
          opened are shown. {relations.error}
        </Notice>
      );
    default:
      return null;
  }
}

function ConnectionItem({
  connection,
  origin,
  other,
  workspace,
  relations,
  byId,
}: {
  connection: Connection;
  origin: DocumentRecord;
  other: DocumentRecord;
  workspace: WorkspaceState;
  relations: RelationshipsState;
  byId: Map<string, DocumentRecord>;
}) {
  const label = describeConnection(connection);
  return (
    <li className="connection">
      <button
        type="button"
        className="related-item"
        onClick={() => relations.openRelated(origin, other)}
      >
        <FileTypeIcon mediaType={other.mediaType} size={20} />
        <span className="related-text">
          <span className="related-name">{other.name}</span>
          <span className="related-path" title={other.relativePath}>
            {originalLocation(other, workspace)}
          </span>
        </span>
        <Badge>{label.type}</Badge>
      </button>
      <p className="connection-provenance">{label.provenance}</p>
      <ConnectionEvidence
        evidence={connection.evidence}
        byId={byId}
        onOpen={relations.openPassage}
      />
    </li>
  );
}

/** Every document connected to `document`, with type, evidence and location. */
export function RelatedList({
  document,
  workspace,
  relations,
}: {
  document: DocumentRecord;
  workspace: WorkspaceState;
  relations: RelationshipsState;
}) {
  const byId = new Map(workspace.documents.map((item) => [item.id, item]));
  const connections = relations
    .connectionsOf(document.id)
    .filter((connection) => byId.has(connection.otherId));

  return (
    <div className="related">
      <CoverageNote relations={relations} />
      {connections.length ? (
        <>
          <p className="muted">
            {connections.length} related{" "}
            {connections.length === 1 ? "file" : "files"}
          </p>
          <ul className="related-list">
            {connections.map((connection) => (
              <ConnectionItem
                key={`${connection.kind}-${connection.otherId}`}
                connection={connection}
                origin={document}
                other={byId.get(connection.otherId)!}
                workspace={workspace}
                relations={relations}
                byId={byId}
              />
            ))}
          </ul>
        </>
      ) : (
        <EmptyState title="No related files found">
          Folio connects files through links written inside them and identical
          copies.
        </EmptyState>
      )}
    </div>
  );
}
