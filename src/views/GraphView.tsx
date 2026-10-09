import { ArrowLeftRight, ArrowRight, Waypoints } from "lucide-react";
import { useEffect } from "react";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import { describeConnection, type Connection } from "../domain/connections";
import type { DocumentRecord } from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { EmptyState } from "../ui/EmptyState";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { Panel } from "../ui/Panel";
import {
  ConnectionEvidence,
  CoverageNote,
  originalLocation,
} from "./Connections";

interface Pair {
  from: DocumentRecord;
  to: DocumentRecord;
  connection: Connection;
}

/** Each connection once: seen from the file that comes first by path. */
function allPairs(
  documents: DocumentRecord[],
  relations: RelationshipsState,
): Pair[] {
  const sorted = [...documents].sort((a, b) =>
    a.relativePath.localeCompare(b.relativePath),
  );
  const order = new Map(sorted.map((document, index) => [document.id, index]));
  const byId = new Map(sorted.map((document) => [document.id, document]));
  const pairs: Pair[] = [];
  for (const from of sorted)
    for (const connection of relations.connectionsOf(from.id)) {
      const to = byId.get(connection.otherId);
      if (to && order.get(from.id)! < order.get(to.id)!)
        pairs.push({ from, to, connection });
    }
  return pairs;
}

function FileEnd({
  document,
  workspace,
  onOpen,
}: {
  document: DocumentRecord;
  workspace: WorkspaceState;
  onOpen: () => void;
}) {
  return (
    <button type="button" className="connection-end" onClick={onOpen}>
      <FileTypeIcon mediaType={document.mediaType} size={20} />
      <span className="related-text">
        <span className="related-name">{document.name}</span>
        <span className="related-path" title={document.relativePath}>
          {originalLocation(document, workspace)}
        </span>
      </span>
    </button>
  );
}

/**
 * The relationships of the whole workspace as a keyboard- and screen-reader-
 * friendly list. A drawn graph would be an extra view, never the only one.
 */
export function GraphView({
  workspace,
  relations,
}: {
  workspace: WorkspaceState;
  relations: RelationshipsState;
}) {
  const { request } = relations;
  useEffect(request, [request]);
  const pairs = allPairs(workspace.documents, relations);
  const byId = new Map(workspace.documents.map((d) => [d.id, d]));

  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Graph</h1>
        <p className="page-tagline">
          How your files connect, with the evidence.
        </p>
      </header>
      <Panel
        title="Connections between files"
        actions={
          <>
            {workspace.source === "samples" && <Badge>Sample files</Badge>}
            <Badge>{pairs.length} found</Badge>
          </>
        }
      >
        <CoverageNote relations={relations} />
        {pairs.length ? (
          <ul className="relationship-list">
            {pairs.map(({ from, to, connection }) => {
              const label = describeConnection(connection);
              const directed =
                connection.direction === "outgoing" ||
                connection.direction === "incoming";
              const [first, second] =
                connection.direction === "incoming" ? [to, from] : [from, to];
              return (
                <li
                  key={`${connection.kind}-${from.id}-${to.id}`}
                  className="connection connection-pair"
                >
                  <div className="connection-ends">
                    <FileEnd
                      document={first}
                      workspace={workspace}
                      onOpen={() => workspace.selectDocument(first)}
                    />
                    {directed ? (
                      <ArrowRight size={16} aria-label="links to" />
                    ) : (
                      <ArrowLeftRight size={16} aria-label="and" />
                    )}
                    <FileEnd
                      document={second}
                      workspace={workspace}
                      onOpen={() => workspace.selectDocument(second)}
                    />
                  </div>
                  <p className="connection-provenance">
                    <Badge>
                      {connection.kind === "explicitReference"
                        ? "Link"
                        : label.type}
                    </Badge>{" "}
                    {label.provenance}
                  </p>
                  <ConnectionEvidence
                    evidence={connection.evidence}
                    byId={byId}
                    onOpen={relations.openPassage}
                  />
                </li>
              );
            })}
          </ul>
        ) : (
          <EmptyState
            icon={<Waypoints size={24} />}
            title="No connections found"
          >
            Folio connects files through links written inside them and identical
            copies.
          </EmptyState>
        )}
      </Panel>
    </div>
  );
}
