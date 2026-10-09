import {
  FlaskConical,
  Folder,
  Folders,
  House,
  Monitor,
  Moon,
  Sparkles,
  Sun,
  Waypoints,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  applyTheme,
  loadTheme,
  nextTheme,
  saveTheme,
  THEME_LABELS,
  type ThemePreference,
} from "../app/theme";
import { useRelationships } from "../app/useRelationships";
import { useWorkspace, type WorkspaceSourceKind } from "../app/useWorkspace";
import { simulatedCode } from "../adapters/simulate";
import { useDrafts } from "../app/drafts";
import { RECOVERY } from "../app/recovery";
import { AnnouncerProvider } from "../ui/Announcer";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { Notice } from "../ui/Notice";
import { SearchField } from "../ui/SearchField";
import { AssistantView } from "../views/AssistantView";
import { DocumentPanel } from "../views/DocumentPanel";
import { FilesView } from "../views/FilesView";
import { GraphView } from "../views/GraphView";
import { HomeView } from "../views/HomeView";
import { ModelLabView } from "../views/ModelLabView";
import { OrganizeView } from "../views/OrganizeView";
import {
  currentPlatform,
  isSearchShortcut,
  PRIMARY_NAV,
  searchShortcutLabel,
  SECONDARY_NAV,
  type NavItem,
  type ViewId,
} from "./navigation";
import { readerDocument } from "./reader";

const ICONS: Record<ViewId, LucideIcon> = {
  home: House,
  files: Folder,
  organize: Folders,
  graph: Waypoints,
  assistant: Sparkles,
  modelLab: FlaskConical,
};

const THEME_ICONS: Record<ThemePreference, LucideIcon> = {
  system: Monitor,
  light: Sun,
  dark: Moon,
};

const SOURCE_LABELS: Record<WorkspaceSourceKind, string> = {
  none: "No folder yet",
  samples: "Sample files",
  folder: "Your folder",
};

const TITLES: Record<ViewId, string> = {
  home: "Overview",
  files: "Files",
  organize: "Organize",
  graph: "Graph",
  assistant: "Ask & Act",
  modelLab: "Model Lab",
};

/** Escape in a text field belongs to the field (a search box clears itself). */
function isEditable(target: EventTarget | null) {
  return (
    target instanceof Element &&
    target.closest("input, textarea, select, [contenteditable='true']") !== null
  );
}

export function AppShell() {
  const workspace = useWorkspace();
  const drafts = useDrafts();
  const relations = useRelationships(workspace);
  const [view, setView] = useState<ViewId>("home");
  const searchInput = useRef<HTMLInputElement>(null);
  const platform = useMemo(currentPlatform, []);
  const [theme, setTheme] = useState<ThemePreference>(loadTheme);
  const reading = readerDocument(view, workspace.selected, workspace.results);
  const showsDocument = reading !== undefined;

  // The listener is added once and reads the latest render through this ref.
  const latest = useRef({ showsDocument, closeDocument });
  latest.current = { showsDocument, closeDocument };

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        // An open modal handles its own Escape.
        if (
          latest.current.showsDocument &&
          !event.defaultPrevented &&
          !isEditable(event.target) &&
          !document.querySelector("dialog[open]")
        )
          latest.current.closeDocument();
        return;
      }
      if (!isSearchShortcut(event, platform)) return;
      event.preventDefault();
      searchInput.current?.focus();
      searchInput.current?.select();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [platform]);

  function cycleTheme() {
    const next = nextTheme(theme);
    setTheme(next);
    saveTheme(next);
    applyTheme(next, document.documentElement);
  }
  const ThemeIcon = THEME_ICONS[theme];
  const themeLabel = `Theme: ${THEME_LABELS[theme]}`;
  const themeAction = `${themeLabel}. Switch to ${THEME_LABELS[nextTheme(theme)]}.`;

  // Closing the reader returns focus to the list row that opened it.
  function closeDocument() {
    const id = workspace.selected?.id;
    if (!id) return;
    workspace.clearSelection();
    requestAnimationFrame(() =>
      document
        .querySelector<HTMLElement>(`[data-document-id="${CSS.escape(id)}"]`)
        ?.focus(),
    );
  }

  function onSearch(query: string) {
    workspace.setQuery(query);
    // In narrow windows the reader covers the list; show the results instead.
    if (window.matchMedia("(max-width: 860px)").matches)
      workspace.clearSelection();
    if (view !== "home" && view !== "files") setView("home");
  }

  return (
    <AnnouncerProvider>
      <div className={`app${showsDocument ? " has-document" : ""}`}>
        <a className="skip-link" href="#main">
          Skip to content
        </a>
        <aside className="sidebar">
          {/* Interim text wordmark until the logo SVG is exported. */}
          <span className="wordmark" role="img" aria-label="Folio">
            <span className="wordmark-full" aria-hidden="true">
              folio
            </span>
            <span className="wordmark-short" aria-hidden="true">
              f
            </span>
          </span>
          <nav aria-label="Main" className="nav">
            <NavList items={PRIMARY_NAV} current={view} onSelect={setView} />
          </nav>
          <div className="sidebar-footer">
            <nav aria-label="Settings" className="nav">
              <NavList
                items={SECONDARY_NAV}
                current={view}
                onSelect={setView}
              />
            </nav>
            <button
              type="button"
              className="nav-item theme-switch"
              aria-label={themeAction}
              title={themeAction}
              onClick={cycleTheme}
            >
              <ThemeIcon size={20} aria-hidden="true" />
              <span className="nav-label" aria-hidden="true">
                {themeLabel}
              </span>
            </button>
            <button
              type="button"
              className="status-pill"
              onClick={() => setView("modelLab")}
              aria-label="Local AI not set up. Open Model Lab."
              title="Local AI isn't set up. Open Model Lab."
            >
              <span className="status-dot" aria-hidden="true" />
              <span className="status-text">Local AI not set up</span>
            </button>
          </div>
        </aside>

        <div className="main-column">
          <header className="topbar">
            <nav aria-label="Breadcrumb" className="breadcrumb">
              <span>{SOURCE_LABELS[workspace.source]}</span>
              <span aria-hidden="true">/</span>
              <span aria-current="page">{TITLES[view]}</span>
            </nav>
            <SearchField
              ref={searchInput}
              label="Search files"
              value={workspace.query}
              onChange={onSearch}
              placeholder="Search files, ideas, or projects"
              shortcut={searchShortcutLabel(platform)}
            />
          </header>

          <main id="main" className="main" tabIndex={-1}>
            {simulatedCode && (
              <Notice tone="info">
                Practice mode: this preview simulates “
                {RECOVERY[simulatedCode].title}” so its message can be checked.
                Nothing here is a real problem.
              </Notice>
            )}
            {workspace.failure && (
              <RecoveryNotice
                error={workspace.failure.error}
                actions={{
                  retry: workspace.failure.retry,
                  chooseFolder: workspace.canChooseFolder
                    ? () => void workspace.selectFolder()
                    : undefined,
                  openModelLab: () => setView("modelLab"),
                }}
                onDismiss={workspace.dismissFailure}
              />
            )}
            {workspace.folderAction.status === "succeeded" && (
              <Notice
                tone="success"
                action={
                  <button
                    type="button"
                    className="link-button"
                    onClick={workspace.dismissFolderResult}
                  >
                    Dismiss
                  </button>
                }
              >
                Opened “{workspace.folderAction.result.name}”.{" "}
                {workspace.folderAction.result.files === 1
                  ? "1 file is listed."
                  : `${workspace.folderAction.result.files} files are listed.`}
              </Notice>
            )}
            {workspace.notice && (
              <Notice
                tone="warning"
                action={
                  <button
                    type="button"
                    className="link-button"
                    onClick={workspace.dismissNotice}
                  >
                    Dismiss
                  </button>
                }
              >
                {workspace.notice}
              </Notice>
            )}
            {/* Each view announces into its own live region, which is
                replaced when the view changes, so one workflow's messages
                never surface in another. */}
            <AnnouncerProvider key={view}>
              {view === "home" && (
                <HomeView workspace={workspace} onNavigate={setView} />
              )}
              {view === "files" && <FilesView workspace={workspace} />}
              {view === "organize" && (
                <OrganizeView workspace={workspace} drafts={drafts} />
              )}
              {view === "graph" && (
                <GraphView workspace={workspace} relations={relations} />
              )}
              {view === "assistant" && (
                <AssistantView drafts={drafts} onNavigate={setView} />
              )}
              {view === "modelLab" && <ModelLabView />}
            </AnnouncerProvider>
          </main>
        </div>

        {reading && (
          <DocumentPanel
            key={reading.id}
            document={reading}
            workspace={workspace}
            relations={relations}
            onClose={closeDocument}
            onNavigate={setView}
          />
        )}
      </div>
    </AnnouncerProvider>
  );
}

function NavList({
  items,
  current,
  onSelect,
}: {
  items: NavItem[];
  current: ViewId;
  onSelect: (view: ViewId) => void;
}) {
  return (
    <ul className="nav-list">
      {items.map((item) => {
        const Icon = ICONS[item.id];
        const active = item.id === current;
        return (
          <li key={item.id}>
            <button
              type="button"
              className={`nav-item${active ? " is-active" : ""}`}
              aria-current={active ? "page" : undefined}
              aria-label={item.label}
              title={item.label}
              onClick={() => onSelect(item.id)}
            >
              <Icon size={20} aria-hidden="true" />
              <span className="nav-label" aria-hidden="true">
                {item.label}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
