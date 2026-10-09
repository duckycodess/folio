import {
  FlaskConical,
  History,
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
import { useActivity } from "../app/useActivity";
import { useHome } from "../app/useHome";
import { hasFilters, passesFilters } from "../domain/homeFilters";
import { useWorkspace, type WorkspaceSourceKind } from "../app/useWorkspace";
import { simulatedCode } from "../adapters/simulate";
import { useDrafts } from "../app/drafts";
import { useOrganize } from "../app/useOrganize";
import { RECOVERY } from "../app/recovery";
import type { DocumentRecord } from "../domain/contracts";
import { AnnouncerProvider } from "../ui/Announcer";
import type { RowMenuItem } from "../ui/RowMenu";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { Notice } from "../ui/Notice";
import { AssistantView } from "../views/AssistantView";
import { DocumentPanel } from "../views/DocumentPanel";
import {
  FileActionDialog,
  type FileActionKind,
} from "../views/FileActionDialog";
import { GraphView } from "../views/GraphView";
import { HomeView } from "../views/HomeView";
import { ActivityView } from "../views/ActivityView";
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
  organize: Folders,
  graph: Waypoints,
  assistant: Sparkles,
  activity: History,
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
  organize: "Organize",
  graph: "Graph",
  assistant: "Ask & Act",
  activity: "Activity",
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
  // Read whenever the folder changes, so Activity is current when opened.
  // An Undo from Activity changes files too: re-read the index's links.
  const activity = useActivity(workspace, relations.refresh);
  // After Folio changes files: re-read the index's links and the history.
  const filesChanged = () => {
    relations.refresh();
    activity.reload();
  };
  const home = useHome(workspace);
  // Above the views, so an apply in progress survives switching views.
  const organize = useOrganize(workspace, filesChanged);
  // Home's Rename and Move have their own plan, so they never show up in
  // Organize (and the reverse).
  const fileAction = useOrganize(workspace, filesChanged);
  const [actionDialog, setActionDialog] = useState<{
    kind: FileActionKind;
    document: DocumentRecord;
  } | null>(null);
  // "Show related" opens the document on its Related tab, and Ask & Act
  // can open one on its Summary tab.
  const [panelTab, setPanelTab] = useState<{
    documentId: string;
    tab: "Related" | "Summary";
    /** Reopens the panel on that tab even if the file is already open. */
    request: number;
  } | null>(null);
  // Set by ⌘K / Ctrl K on another page; Home focuses search once it shows.
  const focusSearch = useRef(false);
  const [view, setView] = useState<ViewId>("home");
  const searchInput = useRef<HTMLInputElement>(null);
  const platform = useMemo(currentPlatform, []);
  const [theme, setTheme] = useState<ThemePreference>(loadTheme);
  // Home's filters narrow its list, so the reader follows them there too.
  const listed = useMemo(() => {
    if (view !== "home" || !hasFilters(home.filters)) return workspace.results;
    const now = Date.now();
    return workspace.results.filter((result) =>
      passesFilters(result.document, home.filters, now),
    );
  }, [view, home.filters, workspace.results]);
  const reading = readerDocument(view, workspace.selected, listed);
  const showsDocument = reading !== undefined;

  // "Show related" is for that one opening: once another file (or none) is
  // shown, opening the file again starts on its usual tab.
  const readingId = reading?.id;
  useEffect(() => {
    setPanelTab((current) =>
      current && current.documentId !== readingId ? null : current,
    );
  }, [readingId]);

  // The listener is added once and reads the latest render through this ref.
  const latest = useRef({ showsDocument, closeDocument, openHome });
  latest.current = { showsDocument, closeDocument, openHome };

  function openHome() {
    if (view === "home") focusHomeSearch();
    else setView("home");
  }

  function focusHomeSearch() {
    focusSearch.current = false;
    searchInput.current?.focus();
    searchInput.current?.select();
  }

  useEffect(() => {
    if (view === "home" && focusSearch.current) focusHomeSearch();
  }, [view]);

  function fileActions(document: DocumentRecord): RowMenuItem[] {
    return [
      {
        id: "open",
        label: "Open",
        onSelect: () => void workspace.selectDocument(document),
      },
      {
        id: "rename",
        label: "Rename…",
        onSelect: () => setActionDialog({ kind: "rename", document }),
      },
      {
        id: "move",
        label: "Move to folder…",
        onSelect: () => setActionDialog({ kind: "move", document }),
      },
      {
        id: "related",
        label: "Show related",
        onSelect: () => {
          setPanelTab((current) => ({
            documentId: document.id,
            tab: "Related",
            request: (current?.request ?? 0) + 1,
          }));
          void workspace.selectDocument(document);
        },
      },
    ];
  }

  function closeActionDialog() {
    const id = actionDialog?.document.id;
    setActionDialog(null);
    // Back to the row's ⋯ menu, or the list if the file was renamed away.
    requestAnimationFrame(() => {
      const row = id
        ? document.querySelector<HTMLElement>(
            `[data-row-id="${CSS.escape(id)}"] .row-menu-button`,
          )
        : null;
      (
        row ??
        document.querySelector<HTMLElement>(
          ".file-list .list-row[tabindex='0']",
        )
      )?.focus();
    });
  }

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
      // Search lives on Home (#43): go there, then focus it.
      focusSearch.current = true;
      latest.current.openHome();
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
    setPanelTab(null);
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
                <HomeView
                  workspace={workspace}
                  onNavigate={setView}
                  searchRef={searchInput}
                  searchShortcut={searchShortcutLabel(platform)}
                  onSearch={onSearch}
                  fileActions={fileActions}
                  onOpenPassage={relations.openPassage}
                  home={home}
                />
              )}
              {view === "organize" && (
                <OrganizeView workspace={workspace} organize={organize} />
              )}
              {view === "graph" && (
                <GraphView workspace={workspace} relations={relations} />
              )}
              {view === "assistant" && (
                <AssistantView
                  workspace={workspace}
                  relations={relations}
                  drafts={drafts}
                  onNavigate={setView}
                  onOpenFile={(document, tab) => {
                    if (tab)
                      setPanelTab((current) => ({
                        documentId: document.id,
                        tab,
                        request: (current?.request ?? 0) + 1,
                      }));
                    void workspace.selectDocument(document);
                  }}
                />
              )}
              {view === "activity" && (
                <ActivityView workspace={workspace} activity={activity} />
              )}
              {view === "modelLab" && <ModelLabView />}
            </AnnouncerProvider>
          </main>
        </div>

        {reading && (
          <DocumentPanel
            key={
              panelTab?.documentId === reading.id
                ? `${reading.id}#${panelTab.request}`
                : reading.id
            }
            document={reading}
            workspace={workspace}
            relations={relations}
            initialTab={
              panelTab?.documentId === reading.id ? panelTab.tab : undefined
            }
            actions={fileActions(reading).filter((item) => item.id !== "open")}
            onClose={closeDocument}
            onNavigate={setView}
          />
        )}
        {actionDialog && (
          <FileActionDialog
            kind={actionDialog.kind}
            document={actionDialog.document}
            workspace={workspace}
            drafts={drafts}
            action={fileAction}
            onClose={closeActionDialog}
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
