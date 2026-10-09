import { Folders, SearchX } from "lucide-react";
import { useMemo, type Ref } from "react";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord, SourcePassage } from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Olio } from "../ui/Olio";
import { Panel } from "../ui/Panel";
import { simulatedFailure } from "../adapters/simulate";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import type { RowMenuItem } from "../ui/RowMenu";
import { SearchField } from "../ui/SearchField";
import { FileList } from "./FileList";
import { FolderSearchStatus, ResultEvidence } from "./SearchEvidence";
import { EmptyFolder, NoFolder } from "./NoFolder";
import { WorkspaceSource } from "./WorkspaceSource";

interface HomeViewProps {
  workspace: WorkspaceState;
  onNavigate: (view: ViewId) => void;
  /** The search field, so ⌘K / Ctrl K can focus it from any page. */
  searchRef: Ref<HTMLInputElement>;
  searchShortcut: string;
  onSearch: (query: string) => void;
  /** Each file row's ⋯ menu. */
  fileActions: (document: DocumentRecord) => RowMenuItem[];
  /** Opens the reader at a search excerpt, highlighted. */
  onOpenPassage: (passage: SourcePassage) => void;
}

/**
 * Home is Folio's file browser: search, collections and every file, with each
 * file's actions on its row (#42, #43).
 */
export function HomeView(props: HomeViewProps) {
  const { workspace } = props;
  const searching = workspace.query.trim().length > 0;
  // Every file while not searching; the ranked matches while searching.
  const documents = useMemo(() => {
    const listed = workspace.results.map((result) => result.document);
    return searching
      ? listed
      : listed.sort((a, b) => a.relativePath.localeCompare(b.relativePath));
  }, [searching, workspace.results]);
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

      {!noFolder && (
        // Kept on screen in narrow windows while a document is open.
        <div className="home-search" role="search">
          <SearchField
            ref={props.searchRef}
            label="Search files"
            value={workspace.query}
            onChange={props.onSearch}
            placeholder="Search files, ideas, or projects"
            shortcut={props.searchShortcut}
          />
        </div>
      )}

      {noFolder ? (
        <NoFolder workspace={workspace} />
      ) : (
        <HomeContents {...props} searching={searching} documents={documents} />
      )}
    </div>
  );
}

function HomeContents({
  workspace,
  onNavigate,
  fileActions,
  onOpenPassage,
  onSearch,
  searching,
  documents,
}: HomeViewProps & { searching: boolean; documents: DocumentRecord[] }) {
  const query = workspace.query.trim();
  // A file kept open from before the search, which the search leaves out.
  const openElsewhere =
    searching &&
    workspace.selected &&
    !documents.some((document) => document.id === workspace.selected?.id)
      ? workspace.selected
      : undefined;
  const status = !searching
    ? ""
    : workspace.search.searching
      ? "Searching…"
      : `${documents.length} ${documents.length === 1 ? "file matches" : "files match"} your search.`;

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
      {searching && (
        <FolderSearchStatus workspace={workspace} onNavigate={onNavigate} />
      )}
      <Panel
        title={searching ? "Search results" : "Files"}
        actions={
          <>
            {searching && <Badge>Keyword search</Badge>}
            {documents.length > 0 && (
              <Badge>
                {documents.length} {documents.length === 1 ? "file" : "files"}
              </Badge>
            )}
          </>
        }
      >
        <p className="visually-hidden" role="status">
          {status}
        </p>
        {openElsewhere && (
          <p className="muted search-kept">
            “{openElsewhere.name}” is still open, but it doesn't match this
            search.{" "}
            <button
              type="button"
              className="link-button"
              onClick={() => onSearch("")}
            >
              Clear search
            </button>
          </p>
        )}
        {documents.length ? (
          <FileList
            label={searching ? "Search results" : "Files"}
            documents={documents}
            selectedId={workspace.selected?.id}
            onSelect={workspace.selectDocument}
            actions={fileActions}
            results={workspace.results}
            renderDetail={
              searching
                ? (_document, result) =>
                    result && (
                      <ResultEvidence
                        result={result}
                        query={query}
                        onOpenPassage={onOpenPassage}
                      />
                    )
                : undefined
            }
          />
        ) : searching ? (
          <EmptyState icon={<SearchX size={24} />} title="No matching files">
            {workspace.search.searching
              ? "Searching…"
              : workspace.source === "folder" &&
                  workspace.search.index !== "ready"
                ? `No file name contains “${query}”. Index this folder to search inside files.`
                : `No file contains “${query}”. Try another word.`}
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
        {searching && (
          <p className="muted search-scope-note">
            Keyword search finds the words you type. Finding files by meaning,
            across English and Filipino, needs a local AI model.{" "}
            <button
              type="button"
              className="link-button"
              onClick={() => onNavigate("modelLab")}
            >
              Open Model Lab
            </button>
          </p>
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
