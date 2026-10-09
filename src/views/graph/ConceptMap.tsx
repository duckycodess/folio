import { useMemo, useState } from "react";
import type { RelationshipsState } from "../../app/useRelationships";
import type { WorkspaceState } from "../../app/useWorkspace";
import { connectionsFor, type ConnectionKind } from "../../domain/connections";
import {
  buildGraph,
  filterEdges,
  mapSubset,
  MAX_MAP_NODES,
  type GraphPair,
} from "../../domain/graph";
import { useAnnounce } from "../../ui/Announcer";
import { Notice } from "../../ui/Notice";
import { ConnectionItem } from "../Connections";
import { GraphCanvas } from "./GraphCanvas";
import { GraphLegend } from "./GraphLegend";

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

/**
 * The map, its legend and the selected file's connections with evidence. It
 * is drawn from the same pairs as the list, which stays one click away.
 */
export function ConceptMap({
  workspace,
  relations,
  pairs,
}: {
  workspace: WorkspaceState;
  relations: RelationshipsState;
  pairs: GraphPair[];
}) {
  const announce = useAnnounce();
  const { documents, selected } = workspace;
  const graph = useMemo(() => buildGraph(documents, pairs), [documents, pairs]);
  const large = graph.nodes.length > MAX_MAP_NODES;
  const centre = large ? (selected?.id ?? null) : null;
  const subset = useMemo(() => mapSubset(graph, centre), [graph, centre]);
  const [hidden, setHidden] = useState<ReadonlySet<ConnectionKind>>(
    () => new Set(),
  );
  const shown = useMemo(
    () => filterEdges(subset.graph, hidden),
    [subset.graph, hidden],
  );
  const counts = useMemo(() => {
    const result: Partial<Record<ConnectionKind, number>> = {};
    for (const edge of graph.edges)
      result[edge.kind] = (result[edge.kind] ?? 0) + 1;
    return result;
  }, [graph]);
  const byId = useMemo(
    () => new Map(documents.map((document) => [document.id, document])),
    [documents],
  );
  const centreName = subset.centerId ? byId.get(subset.centerId)?.name : null;

  function toggle(kind: ConnectionKind) {
    setHidden((current) => {
      const next = new Set(current);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
  }

  function open(id: string) {
    const document = byId.get(id);
    if (!document) return;
    void workspace.selectDocument(document);
    announce(`Opened ${document.name} in the reader.`);
  }

  function close() {
    const name = workspace.selected?.name;
    workspace.clearSelection();
    if (name) announce(`Closed ${name}.`);
  }

  const selectedOnMap =
    selected && shown.nodes.some((node) => node.id === selected.id)
      ? selected
      : null;
  const connections = selectedOnMap
    ? connectionsFor(
        selectedOnMap.id,
        relations.relationships,
        relations.duplicates,
      ).filter((connection) => byId.has(connection.otherId))
    : [];
  const summary = `${plural(shown.nodes.length, "file", "files")}, ${plural(
    shown.edges.length,
    "connection",
    "connections",
  )} shown`;

  return (
    <div className="concept-map">
      {large && (
        <Notice tone="info">
          This folder has {graph.nodes.length} files, more than the map can show
          clearly. It shows the files around {centreName ?? "one file"}
          {subset.omitted ? ` (${subset.omitted} files left out)` : ""}. Open
          another file to move the map; the List shows every connection.
        </Notice>
      )}
      <p className="muted tabular" aria-hidden="true">
        {summary}
      </p>
      <GraphCanvas
        base={subset.graph}
        shown={shown}
        selectedId={selected?.id ?? null}
        label={`Concept map: ${summary}`}
        onOpen={open}
        onClose={close}
      />
      <GraphLegend counts={counts} hidden={hidden} onToggle={toggle} />
      {selectedOnMap && (
        <section
          className="graph-selection"
          aria-label={`Connections of ${selectedOnMap.name}`}
        >
          <h3 className="subsection-title">
            {selectedOnMap.name}:{" "}
            {plural(connections.length, "connection", "connections")}
          </h3>
          {connections.length ? (
            <ul className="related-list">
              {connections.map((connection) => (
                <ConnectionItem
                  key={`${connection.kind}-${connection.otherId}`}
                  connection={connection}
                  origin={selectedOnMap}
                  other={byId.get(connection.otherId)!}
                  workspace={workspace}
                  relations={relations}
                  byId={byId}
                />
              ))}
            </ul>
          ) : (
            <p className="muted">
              Folio found no connections to or from this file.
            </p>
          )}
        </section>
      )}
    </div>
  );
}
