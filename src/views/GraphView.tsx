import {
  ArrowLeftRight,
  ArrowRight,
  List,
  Network,
  Waypoints,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import { describeConnection } from "../domain/connections";
import type { DocumentRecord } from "../domain/contracts";
import { graphPairs, type GraphPair } from "../domain/graph";
import { Badge } from "../ui/Badge";
import { EmptyState } from "../ui/EmptyState";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { Panel } from "../ui/Panel";
import {
  ConnectionEvidence,
  CoverageNote,
  originalLocation,
} from "./Connections";
import { ConceptMap } from "./graph/ConceptMap";

type GraphMode = "map" | "list";

/** The last Map/List choice, kept while the app runs. */
let rememberedMode: GraphMode = "map";

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

function ConnectionList({
  pairs,
  workspace,
  relations,
}: {
  pairs: GraphPair[];
  workspace: WorkspaceState;
  relations: RelationshipsState;
}) {
  const byId = new Map(workspace.documents.map((d) => [d.id, d]));
  return (
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
                {connection.kind === "explicitReference" ? "Link" : label.type}
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
  );
}

/**
 * The relationships of the whole workspace, as a concept map or as a
 * keyboard- and screen-reader-friendly list. Both are drawn from the same
 * pairs; the list is an equal alternative, never a fallback.
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
  const { documents } = workspace;
  const { relationships, duplicates } = relations;
  const pairs = useMemo(
    () => graphPairs(documents, relationships, duplicates),
    [documents, relationships, duplicates],
  );
  const [mode, setMode] = useState<GraphMode>(rememberedMode);
  function choose(next: GraphMode) {
    rememberedMode = next;
    setMode(next);
  }

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
          <>
            <div className="segmented" role="group" aria-label="Show as">
              <button
                type="button"
                className="segmented-option"
                aria-pressed={mode === "map"}
                onClick={() => choose("map")}
              >
                <Network size={16} aria-hidden="true" />
                Map
              </button>
              <button
                type="button"
                className="segmented-option"
                aria-pressed={mode === "list"}
                onClick={() => choose("list")}
              >
                <List size={16} aria-hidden="true" />
                List
              </button>
            </div>
            {mode === "map" ? (
              <ConceptMap
                workspace={workspace}
                relations={relations}
                pairs={pairs}
              />
            ) : (
              <ConnectionList
                pairs={pairs}
                workspace={workspace}
                relations={relations}
              />
            )}
          </>
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
