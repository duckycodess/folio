import { Folders, SearchX } from "lucide-react";
import { useMemo, type Ref } from "react";
import type { CollectionsController } from "../app/useCollections";
import type { HomeState } from "../app/useHome";
import type { WorkspaceState } from "../app/useWorkspace";
import { membersLabel } from "../domain/collections";
import {
  foldersOf,
  hasFilters,
  NO_FILTERS,
  passesFilters,
} from "../domain/homeFilters";
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
import { AskOlioLauncher } from "./AskOlioLauncher";
import { FileList } from "./FileList";
import { HomeFilterBar, PinnedFolders, RecentFiles } from "./HomeFilters";
import { FolderSearchStatus, ResultEvidence } from "./SearchEvidence";
import { EmptyFolder, NoFolder } from "./NoFolder";
import { WorkspaceSource } from "./WorkspaceSource";

interface HomeViewProps {
  workspace: WorkspaceState;
  /** Kept collections, listed above the files. */
  collections: CollectionsController;
  onNavigate: (view: ViewId) => void;
  /** The search field, so ⌘K / Ctrl K can focus it from any page. */
  searchRef: Ref<HTMLInputElement>;
  searchShortcut: string;
  onSearch: (query: string) => void;
  /** Each file row's ⋯ menu. */
  fileActions: (document: DocumentRecord) => RowMenuItem[];
  /** Opens the reader at a search excerpt, highlighted. */
  onOpenPassage: (passage: SourcePassage) => void;
  /** Filters, pinned folders and recent files, kept above the views. */
  home: HomeState;
  /** Opens Ask & Act with Home's search and folder filled in. */
  onAskOlio: () => void;
}

/**
 * Home is Folio's file browser: search, collections and every file, with each
 * file's actions on its row (#42, #43).
 */
export function HomeView(props: HomeViewProps) {
  const { workspace } = props;
  const searching = workspace.query.trim().length > 0;
  // Every file while not searching; the ranked matches while searching.
  const matched = useMemo(() => {
    const listed = workspace.results.map((result) => result.document);
    return searching
      ? listed
      : listed.sort((a, b) => a.relativePath.localeCompare(b.relativePath));
  }, [searching, workspace.results]);
  const { filters } = props.home;
  const documents = useMemo(() => {
    const now = Date.now();
    return matched.filter((document) => passesFilters(document, filters, now));
  }, [matched, filters]);
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
          <AskOlioLauncher
            query={workspace.query}
            folder={props.home.filters.folder}
            onOpen={props.onAskOlio}
          />
        </div>
      )}

      {noFolder ? (
        <NoFolder workspace={workspace} />
      ) : (
        <HomeContents
          {...props}
          searching={searching}
          documents={documents}
          unfiltered={matched.length}
        />
      )}
    </div>
  );
}

function HomeContents({
  workspace,
  collections,
  onNavigate,
  fileActions,
  onOpenPassage,
  onSearch,
  home,
  searching,
  documents,
  unfiltered,
}: HomeViewProps & {
  searching: boolean;
  documents: DocumentRecord[];
  /** Matches before the filters, to say what the filters hid. */
  unfiltered: number;
}) {
  const filtering = hasFilters(home.filters);
  const folders = useMemo(
    () => foldersOf(workspace.documents),
    [workspace.documents],
  );
  const recent = home.recentIds
    .map((id) => workspace.documents.find((document) => document.id === id))
    .filter((document): document is DocumentRecord => Boolean(document))
    .slice(0, 5);
  const query = workspace.query.trim();
  // A file kept open from before the search, which the search leaves out.
  const openElsewhere =
    (searching || filtering) &&
    workspace.selected &&
    !documents.some((document) => document.id === workspace.selected?.id)
      ? workspace.selected
      : undefined;
  const status =
    !searching && !filtering
      ? ""
      : workspace.search.searching
        ? "Searching…"
        : `${documents.length} ${documents.length === 1 ? "file matches" : "files match"} ${searching ? "your search" : "the filters"}.`;

  return (
    <>
      {workspace.documents.length > 0 && (
        <HomeFilterBar home={home} folders={folders} />
      )}
      <WorkspaceSource workspace={workspace} />
      <PinnedFolders home={home} folders={folders} />
      {!searching && !filtering && (
        <RecentFiles documents={recent} onOpen={workspace.selectDocument} />
      )}

      {collections.collections.length > 0 && !searching && !filtering && (
        <section className="section" aria-labelledby="collections-heading">
          <h2 id="collections-heading" className="section-title">
            Collections
          </h2>
          <ul className="home-collections">
            {collections.collections.map((collection) => (
              <li key={collection.id} className="home-collection">
                <Folders size={16} aria-hidden="true" />
                <span className="home-collection-name">{collection.name}</span>
                <span className="muted">{membersLabel(collection)}</span>
              </li>
            ))}
          </ul>
          <Button variant="ghost" onClick={() => onNavigate("organize")}>
            Manage collections in Organize
          </Button>
        </section>
      )}

      {/* Empty, so it gives way to the file list in short windows, and to
          pinned folders and recent files once there are some. */}
      {!collections.collections.length &&
        !home.pins.length &&
        !(recent.length && !searching && !filtering) && (
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
        )}

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
                {filtering
                  ? `${documents.length} of ${unfiltered} ${unfiltered === 1 ? "file" : "files"}`
                  : `${documents.length} ${documents.length === 1 ? "file" : "files"}`}
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
            “{openElsewhere.name}” is still open, but{" "}
            {searching ? "it doesn't match this search" : "the filters hide it"}
            .{" "}
            <button
              type="button"
              className="link-button"
              onClick={() =>
                searching ? onSearch("") : home.setFilters(NO_FILTERS)
              }
            >
              {searching ? "Clear search" : "Clear filters"}
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
        ) : filtering && unfiltered > 0 ? (
          <EmptyState
            icon={<SearchX size={24} />}
            title="No files match these filters"
            action={
              <Button
                variant="secondary"
                onClick={() => home.setFilters(NO_FILTERS)}
              >
                Clear filters
              </Button>
            }
          >
            {unfiltered === 1
              ? "1 file is hidden by the filters."
              : `${unfiltered} files are hidden by the filters.`}
          </EmptyState>
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
