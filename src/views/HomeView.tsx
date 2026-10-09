import { Folders, SearchX } from "lucide-react";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord } from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Olio } from "../ui/Olio";
import { Panel } from "../ui/Panel";
import { simulatedFailure } from "../adapters/simulate";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { FileList } from "./FileList";
import { EmptyFolder, NoFolder } from "./NoFolder";
import { WorkspaceSource } from "./WorkspaceSource";

interface HomeViewProps {
  workspace: WorkspaceState;
  onNavigate: (view: ViewId) => void;
}

export function HomeView({ workspace, onNavigate }: HomeViewProps) {
  const searching = workspace.query.trim().length > 0;
  const documents = workspace.results.map((result) => result.document);
  const noFolder = workspace.source === "none";

  return (
    <div className="view">
      <header className="page-header page-header-home">
        {/* One Olio per view: the header pose follows the list below, and
            the no-folder state brings its own. */}
        {!noFolder && (
          <Olio
            pose={
              documents.length ? "default" : searching ? "confused" : "peeking"
            }
            size={96}
          />
        )}
        <div className="page-header-text">
          <h1 className="page-title">Your workspace</h1>
          <p className="page-tagline">Everything in its place.</p>
        </div>
      </header>

      {noFolder ? (
        <NoFolder workspace={workspace} />
      ) : (
        <HomeContents
          workspace={workspace}
          onNavigate={onNavigate}
          searching={searching}
          documents={documents}
        />
      )}
    </div>
  );
}

function HomeContents({
  workspace,
  onNavigate,
  searching,
  documents,
}: HomeViewProps & { searching: boolean; documents: DocumentRecord[] }) {
  return (
    <>
      <WorkspaceSource workspace={workspace} />

      {/* Empty, so it gives way to the file list in short windows. */}
      <section
        className="section home-collections-empty"
        aria-labelledby="collections-heading"
      >
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

      {searching && <SearchProblem onNavigate={onNavigate} />}
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
        ) : workspace.loading ? (
          <p className="muted">Loading files…</p>
        ) : workspace.source === "folder" ? (
          <EmptyFolder workspace={workspace} showOlio={false} />
        ) : (
          <EmptyState title="No sample files">
            The sample files couldn't be loaded.
          </EmptyState>
        )}
      </Panel>
    </>
  );
}

/** Practice mode only: retrieval problems, shown above the keyword results. */
function SearchProblem({ onNavigate }: Pick<HomeViewProps, "onNavigate">) {
  const failure = simulatedFailure("search");
  if (!failure) return null;
  return (
    <RecoveryNotice
      error={failure}
      actions={{ openModelLab: () => onNavigate("modelLab") }}
    />
  );
}
