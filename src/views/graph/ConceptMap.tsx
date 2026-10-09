import { useMemo, useRef, useState } from "react";
import { graphActions, type GraphActionKind } from "../../app/graphActions";
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
import type { DocumentRecord } from "../../domain/contracts";
import { Notice } from "../../ui/Notice";
import { RowMenu } from "../../ui/RowMenu";
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
  onFileAction,
}: {
  workspace: WorkspaceState;
  relations: RelationshipsState;
  pairs: GraphPair[];
  onFileAction?: (kind: GraphActionKind, document: DocumentRecord) => void;
}) {
  const announce = useAnnounce();
  const frame = useRef<HTMLDivElement>(null);
  // Shift+F10 or a right-click on a node asks for its actions menu.
  // `pending` until the menu has opened, so it never reopens later.
  const [actionsRequest, setActionsRequest] = useState({
    id: "",
    count: 0,
    pending: false,
  });
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

  /** Opens a node's actions: it becomes the open file, then its menu opens. */
  function requestActions(id: string) {
    if (workspace.selected?.id !== id) open(id);
    setActionsRequest((current) => ({
      id,
      count: current.count + 1,
      pending: true,
    }));
  }

  /** After a menu opened from the map closes, focus goes back to its node. */
  function focusNode(id: string) {
    frame.current
      ?.querySelector<SVGGElement>(`[data-node-id="${CSS.escape(id)}"]`)
      ?.focus();
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
    <div className="concept-map" ref={frame}>
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
        onActions={onFileAction ? requestActions : undefined}
      />
      <GraphLegend counts={counts} hidden={hidden} onToggle={toggle} />
      {selectedOnMap && (
        <section
          className="graph-selection"
          aria-label={`Connections of ${selectedOnMap.name}`}
        >
          <div className="graph-selection-head">
            <h3 className="subsection-title">
              {selectedOnMap.name}:{" "}
              {plural(connections.length, "connection", "connections")}
            </h3>
            {onFileAction && (
              <RowMenu
                label={`Actions for ${selectedOnMap.name}`}
                tabbable
                items={graphActions(workspace, selectedOnMap).map((action) => ({
                  id: action.kind,
                  label: action.label,
                  disabledReason: action.disabledReason,
                  onSelect: () => onFileAction(action.kind, selectedOnMap),
                }))}
                openRequest={
                  actionsRequest.pending &&
                  actionsRequest.id === selectedOnMap.id
                    ? actionsRequest.count
                    : 0
                }
                onRequestOpened={() =>
                  setActionsRequest((current) => ({
                    ...current,
                    pending: false,
                  }))
                }
                onRequestedClose={() => focusNode(selectedOnMap.id)}
              />
            )}
          </div>
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
