import { Folders, SearchX } from "lucide-react";
import type { WorkspaceState } from "../app/useWorkspace";
import type { ViewId } from "../shell/navigation";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Panel } from "../ui/Panel";
import { FileList } from "./FileList";
import { WorkspaceSource } from "./WorkspaceSource";

interface HomeViewProps {
  workspace: WorkspaceState;
  onNavigate: (view: ViewId) => void;
}

export function HomeView({ workspace, onNavigate }: HomeViewProps) {
  const searching = workspace.query.trim().length > 0;
  const documents = workspace.results.map((result) => result.document);

  return (
    <div className="view">
      <header className="page-header">
        <h1 className="page-title">Your workspace</h1>
        <p className="page-tagline">Everything in its place.</p>
      </header>

      <WorkspaceSource workspace={workspace} />

      <section className="section" aria-labelledby="collections-heading">
        <h2 id="collections-heading" className="section-title">
          Collections
        </h2>
        <EmptyState
          compact
          icon={<Folders size={20} />}
          title="No collections yet"
          action={
            <Button variant="ghost" onClick={() => onNavigate("organize")}>
              Go to Organize
            </Button>
          }
        >
          Collections group related files without moving them.
        </EmptyState>
      </section>

      <Panel
        title={searching ? "Search results" : "Files"}
        actions={searching && <Badge>Keyword search</Badge>}
      >
        <p className="visually-hidden" role="status">
          {searching
            ? `${documents.length} ${documents.length === 1 ? "file matches" : "files match"} your search.`
            : ""}
        </p>
        {documents.length ? (
          <FileList
            label={searching ? "Search results" : "Files"}
            documents={documents}
            selectedId={workspace.selected?.id}
            onSelect={workspace.selectDocument}
          />
        ) : searching ? (
          <EmptyState icon={<SearchX size={24} />} title="No matching files">
            No file contains “{workspace.query.trim()}”. Try another word.
          </EmptyState>
        ) : (
          <EmptyState title="This folder has no supported files">
            Folio reads text, Markdown and text-based PDF files.
          </EmptyState>
        )}
      </Panel>
    </div>
  );
}
