import type { OrganizeController } from "../app/useOrganize";
import type { WorkspaceState } from "../app/useWorkspace";
import { EmptyState } from "../ui/EmptyState";
import { Olio } from "../ui/Olio";
import { Panel } from "../ui/Panel";
import { OrganizeFlowPanel } from "./OrganizeFlowPanel";

/**
 * Journey B: analyze a folder and act on suggestions. Renaming or moving one
 * file lives on Home's file rows instead (#42).
 */
export function OrganizeView({
  workspace,
  organize,
}: {
  workspace: WorkspaceState;
  organize: OrganizeController;
}) {
  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Organize</h1>
        <p className="page-tagline">
          Find duplicates and clearer names. Nothing changes on disk without
          your approval.
        </p>
      </header>

      <OrganizeFlowPanel workspace={workspace} organize={organize} />

      <Panel title="Collections">
        <EmptyState
          illustration={<Olio pose="organizing" size={96} />}
          title="No collections yet"
        >
          Collections are virtual: they group related files without moving or
          copying them. Creating collections isn't available in this version
          yet.
        </EmptyState>
      </Panel>
    </div>
  );
}
