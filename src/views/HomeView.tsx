import { SearchX } from "lucide-react";
import { useMemo, type Ref } from "react";
import type { HomeState, HomeTab } from "../app/useHome";
import type { WorkspaceState } from "../app/useWorkspace";
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
import { simulatedFailure } from "../adapters/simulate";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import type { RowMenuItem } from "../ui/RowMenu";
import { SearchField } from "../ui/SearchField";
import { FileList } from "./FileList";
import { HomeFilterBar, PinnedFolders } from "./HomeFilters";
import { FolderSearchStatus, ResultEvidence } from "./SearchEvidence";
import { EmptyFolder, NoFolder } from "./NoFolder";

const TABS: [HomeTab, string][] = [
  ["recent", "Recent"],
  ["starred", "Starred"],
  ["all", "All files"],
];

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
  /** Filters, pinned folders and recent files, kept above the views. */
  home: HomeState;
}

/**
 * Home is Folio's file browser, laid out as the brandkit mockup's Home:
 * search, folder cards and the file table, with each file's actions on its
 * row (#42, #43).
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
  const { filters, tab, starIds } = props.home;
  const documents = useMemo(() => {
    const now = Date.now();
    const filtered = matched.filter((document) =>
      passesFilters(document, filters, now),
    );
    // Search results keep their ranking; the tabs only order the browse.
    if (searching) return filtered;
    if (tab === "starred")
      return filtered.filter((document) => starIds.includes(document.id));
    if (tab === "recent")
      return [...filtered].sort(
        (a, b) => (b.modifiedAtMs ?? -1) - (a.modifiedAtMs ?? -1),
      );
    return filtered;
  }, [matched, filters, searching, tab, starIds]);
  const noFolder = workspace.source === "none";

  return (
    <div className="view view-home">
      <header className="page-header page-header-home">
        {/* One Olio per view: the floating Olio launcher provides it now. */}
        <div className="page-header-text">
          <h1 className="page-title">Find your files.</h1>
          <p className="page-tagline">
            A little less searching. A little more doing.
          </p>
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
            placeholder="Search file names"
            shortcut={props.searchShortcut}
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
  const count = filtering
    ? `${documents.length} of ${unfiltered} ${unfiltered === 1 ? "file" : "files"}`
    : `${documents.length} ${documents.length === 1 ? "file" : "files"}`;

  return (
    <>
      {workspace.documents.length > 0 && (
        <HomeFilterBar
          home={home}
          folders={folders}
          onReset={() => {
            home.setFilters(NO_FILTERS);
            onSearch("");
          }}
        />
      )}
      {!searching && (
        <PinnedFolders
          home={home}
          folders={folders}
          documents={workspace.documents}
        />
      )}

      {searching && <SearchProblem onNavigate={onNavigate} />}
      {searching && (
        <FolderSearchStatus workspace={workspace} onNavigate={onNavigate} />
      )}
      <section className="home-files" aria-label="Files">
        {searching ? (
          <div className="section-head home-results-head">
            <h2 className="section-head-title">Search results</h2>
            <Badge>Keyword search</Badge>
          </div>
        ) : (
          <div className="tabs home-tabs" role="group" aria-label="Show">
            {TABS.map(([id, label]) => (
              <button
                key={id}
                type="button"
                className={`tab${home.tab === id ? " is-active" : ""}`}
                aria-pressed={home.tab === id}
                onClick={() => home.setTab(id)}
              >
                {label}
              </button>
            ))}
          </div>
        )}
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
            isStarred={(document) => home.starIds.includes(document.id)}
            onToggleStar={(document) => home.toggleStar(document.id)}
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
        ) : home.tab === "starred" && workspace.documents.length > 0 ? (
          <p className="empty">
            No starred files yet. Star a file to keep it here.
          </p>
        ) : workspace.loading ? (
          <p className="muted">Loading files…</p>
        ) : workspace.source === "folder" ? (
          <EmptyFolder workspace={workspace} />
        ) : (
          <EmptyState title="No sample files">
            The sample files couldn't be loaded.
          </EmptyState>
        )}
        {documents.length > 0 && (
          <p className="file-count">
            {searching ? `${count} · Keyword matching` : count}
            {/* Said plainly wherever the files are only samples. */}
            {workspace.source === "samples" &&
              (workspace.nativeAvailable
                ? " · Sample files. Choose a folder in Settings & style to see your own."
                : " · Sample files. Folder access works in the desktop app.")}
          </p>
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
      </section>
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
