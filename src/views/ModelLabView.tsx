import { EmptyState } from "../ui/EmptyState";
import { Olio } from "../ui/Olio";
import { Panel } from "../ui/Panel";

export function ModelLabView() {
  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Model Lab</h1>
        <p className="page-tagline">
          Set up and compare the local AI models Folio runs on this device.
        </p>
      </header>
      <Panel title="Installed models">
        <EmptyState
          illustration={<Olio pose="sleeping" size={160} />}
          title="No local AI model is set up"
        >
          Browsing, keyword search and reading files work without one. Model
          setup isn't available in this version yet.
        </EmptyState>
      </Panel>
    </div>
  );
}
