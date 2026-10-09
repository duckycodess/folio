import { ArrowRight, Waypoints } from "lucide-react";
import type { WorkspaceState } from "../app/useWorkspace";
import { Badge } from "../ui/Badge";
import { EmptyState } from "../ui/EmptyState";
import { Panel } from "../ui/Panel";

export function GraphView({ workspace }: { workspace: WorkspaceState }) {
  const byId = new Map(workspace.documents.map((d) => [d.id, d]));
  const links = workspace.relationships.filter(
    (edge) => edge.type === "explicitReference",
  );

  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Graph</h1>
        <p className="page-tagline">
          How your files connect, with the evidence.
        </p>
      </header>
      <Panel
        title="Links between files"
        actions={<Badge>{links.length} found</Badge>}
      >
        {links.length ? (
          <ul className="relationship-list">
            {links.map((edge, index) => {
              const source = byId.get(edge.sourceId);
              const target = byId.get(edge.targetId);
              if (!source || !target) return null;
              return (
                <li key={`${edge.sourceId}-${edge.targetId}-${index}`}>
                  <button
                    type="button"
                    className="relationship-item"
                    onClick={() => workspace.selectDocument(source)}
                  >
                    <span className="relationship-ends">
                      <span>{source.name}</span>
                      <ArrowRight size={16} aria-label="links to" />
                      <span>{target.name}</span>
                    </span>
                    <span className="relationship-evidence">
                      {edge.evidence[0]?.text}
                    </span>
                    <Badge>Linked in file</Badge>
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          <EmptyState icon={<Waypoints size={24} />} title="No links found">
            Folio currently shows only links written inside files.
          </EmptyState>
        )}
      </Panel>
    </div>
  );
}
