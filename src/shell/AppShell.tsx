import {
  Compass,
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
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import {
  applyTheme,
  loadTheme,
  nextTheme,
  saveTheme,
  THEME_LABELS,
  type ThemePreference,
} from "../app/theme";
import {
  clampReaderWidth,
  computeShellLayout,
  loadReaderWidth,
  READER_MIN_WIDTH,
  readerMaxWidth,
  saveReaderWidth,
} from "../app/shellLayout";
import { useElementWidth } from "../app/useElementWidth";
import {
  localAiStatus,
  localAiStatusLabel,
  searchModelKey,
} from "../app/localAi";
import { GenerationReadyProvider } from "../app/generationReady";
import { isGenerationReady } from "../app/models";
import { useModels } from "../app/useModels";
import { AiIndexProvider, useAiIndex } from "../app/useAiIndex";
import { ResizeHandle } from "../ui/ResizeHandle";
import { useRelationships } from "../app/useRelationships";
import { useActivity } from "../app/useActivity";
import { useHome } from "../app/useHome";
import {
  loadOnboardingCompleted,
  saveOnboardingCompleted,
} from "../app/onboardingStorage";
import { shouldStartOnboarding } from "../domain/onboarding";
import { OnboardingView } from "../views/OnboardingView";
import { hasFilters, passesFilters } from "../domain/homeFilters";
import { useWorkspace, type WorkspaceSourceKind } from "../app/useWorkspace";
import { simulatedCode } from "../adapters/simulate";
import { useDrafts } from "../app/drafts";
import { useCollections } from "../app/useCollections";
import { useOrganize } from "../app/useOrganize";
import { RECOVERY } from "../app/recovery";
import type { DocumentRecord } from "../domain/contracts";
import { AnnouncerProvider } from "../ui/Announcer";
import type { RowMenuItem } from "../ui/RowMenu";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { Notice } from "../ui/Notice";
import { AssistantView, type OpenFile } from "../views/AssistantView";
import { DocumentPanel } from "../views/DocumentPanel";
import {
  FileActionDialog,
  type FileActionKind,
} from "../views/FileActionDialog";
import { FloatingOlioChat } from "../views/FloatingOlioChat";
import { GraphView } from "../views/GraphView";
import { AddToCollectionDialog } from "../views/AddToCollectionDialog";
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
import folioWordmark from "../assets/brand/folio-wordmark.png";

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
  // Collections follow Folio's own renames, moves and deletions natively.
  const collections = useCollections(workspace);
  // AI connections: coverage and refresh for the active search model. A
  // refresh starts after Local Sync, an applied change or Undo, only when
  // that model is ready.
  const models = useModels();
  const aiIndex = useAiIndex(
    workspace.workspace?.id,
    searchModelKey(models),
    relations.refresh,
  );
  const indexState = workspace.search.index;
  const previousIndexState = useRef(indexState);
  useEffect(() => {
    if (previousIndexState.current === "indexing" && indexState === "ready")
      aiIndex.refresh();
    previousIndexState.current = indexState;
  }, [indexState, aiIndex.refresh]);
  const afterFilesChanged = () => {
    relations.refresh();
    aiIndex.refresh();
    collections.reload();
  };
  // Read whenever the folder changes, so Activity is current when opened.
  // An Undo from Activity changes files too: re-read the index's links and
  // the collections.
  const activity = useActivity(workspace, afterFilesChanged);
  // After Folio changes files: re-read the index's links, the collections
  // and the history.
  const filesChanged = () => {
    afterFilesChanged();
    activity.reload();
  };
  const home = useHome(workspace);
  // One reading of the model store for the sidebar status and the floating
  // chat, so they always agree with each other and with Model Lab.
  const aiStatus = localAiStatus(models);
  // Shared with Graph and plan previews (see `generationReady.tsx`).
  const generationReady =
    models.load === "ready" &&
    isGenerationReady(models.groups, models.setup, models.runtime);
  const aiLabel = localAiStatusLabel(models);
  // First run in the desktop app; reopened from the sidebar's settings.
  const [welcome, setWelcome] = useState(() =>
    shouldStartOnboarding(workspace.nativeAvailable, loadOnboardingCompleted()),
  );
  function finishWelcome(next?: ViewId) {
    saveOnboardingCompleted();
    setWelcome(false);
    if (next) setView(next);
  }
  // Above the views, so an apply in progress survives switching views.
  const organize = useOrganize(workspace, "organize", filesChanged);
  // Home's Rename and Move have their own plan, so they never show up in
  // Organize (and the reverse).
  const fileAction = useOrganize(workspace, "home", filesChanged);
  const [collectionDialog, setCollectionDialog] =
    useState<DocumentRecord | null>(null);
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
  // Home's scroll position, restored when coming back from another page.
  const mainRef = useRef<HTMLElement>(null);
  const homeScroll = useRef(0);
  useLayoutEffect(() => {
    if (view === "home" && mainRef.current)
      mainRef.current.scrollTop = homeScroll.current;
  }, [view]);
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

  // The sidebar, the list and the reader all follow the app's own measured
  // width, not a fixed window breakpoint (#67): a wide window keeps sidebar
  // labels even with the reader open, and a narrow one collapses sooner.
  const [appRef, appWidth] = useElementWidth<HTMLDivElement>(1280);
  const [requestedReaderWidth, setRequestedReaderWidth] =
    useState(loadReaderWidth);
  const layout = computeShellLayout(
    appWidth,
    showsDocument,
    requestedReaderWidth,
  );
  function resizeReader(next: number) {
    // Kept within this window's range, so a drag past the edge doesn't
    // leave a width that jumps open in a larger window later.
    const width = clampReaderWidth(next, appWidth);
    setRequestedReaderWidth(width);
    saveReaderWidth(width);
  }

  // "Show related" is for that one opening: once another file (or none) is
  // shown, opening the file again starts on its usual tab.
  const readingId = reading?.id;
  useEffect(() => {
    setPanelTab((current) =>
      current && current.documentId !== readingId ? null : current,
    );
  }, [readingId]);

  // The listener is added once and reads the latest render through this ref.
  const overlay = layout.readerMode === "overlay";
  function closeOverlay() {
    setPanelTab(null);
    workspace.clearSelection();
  }
  const latest = useRef({
    showsDocument,
    closeDocument,
    openHome,
    overlay,
    closeOverlay,
  });
  latest.current = {
    showsDocument,
    closeDocument,
    openHome,
    overlay,
    closeOverlay,
  };

  function openHome() {
    if (view !== "home") setView("home");
    else if (!overlay) focusHomeSearch();
    // Otherwise the effect below focuses search once the overlay has closed:
    // the list behind it, search included, is inert until then.
  }

  function focusHomeSearch() {
    focusSearch.current = false;
    searchInput.current?.focus();
    searchInput.current?.select();
  }

  useEffect(() => {
    if (view === "home" && !overlay && focusSearch.current) focusHomeSearch();
  }, [view, overlay]);

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
      ...(collections.available && collections.collections.length
        ? [
            {
              id: "collection",
              label: "Add to collection…",
              onSelect: () => setCollectionDialog(document),
            },
          ]
        : []),
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
      // Search lives on Home (#43): go there, then focus it. A reader that
      // overlays the list makes search inert, so it closes first.
      focusSearch.current = true;
      if (latest.current.overlay) latest.current.closeOverlay();
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

  // Shared by Ask & Act and the floating Olio chat (#66): both open a file
  // from a turn's result the same way.
  const openFromAsk: OpenFile = (document, tab) => {
    if (tab)
      setPanelTab((current) => ({
        documentId: document.id,
        tab,
        request: (current?.request ?? 0) + 1,
      }));
    void workspace.selectDocument(document);
  };

  function onSearch(query: string) {
    workspace.setQuery(query);
    // When the reader overlays the list, show the results instead.
    if (layout.readerMode === "overlay") workspace.clearSelection();
  }

  if (welcome)
    return (
      <AnnouncerProvider>
        <OnboardingView
          workspace={workspace}
          relations={relations}
          onFinish={finishWelcome}
        />
      </AnnouncerProvider>
    );

  const appClassName = [
    "app",
    showsDocument && "has-document",
    layout.sidebarMode === "rail" && "sidebar-rail",
    layout.readerMode === "split" && "reader-split",
    layout.readerMode === "overlay" && "reader-overlay",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <AiIndexProvider value={aiIndex}>
      <GenerationReadyProvider value={generationReady}>
        <AnnouncerProvider>
          <div
            ref={appRef}
            className={appClassName}
            style={
              { "--detail-width": `${layout.readerWidth}px` } as CSSProperties
            }
          >
            <a className="skip-link" href="#main">
              Skip to content
            </a>
            <aside className="sidebar">
              {/* The brandkit's gold eye wordmark and motto (docs/design.md). */}
              <div className="brand">
                <img
                  className="wordmark"
                  src={folioWordmark}
                  alt="Folio"
                  width={178}
                  height={64}
                  draggable={false}
                />
                <span className="brand-motto">Search. Organize. Summarize</span>
              </div>
              <nav aria-label="Main" className="nav">
                <NavList
                  items={PRIMARY_NAV}
                  current={view}
                  onSelect={setView}
                />
              </nav>
              <div className="sidebar-footer">
                <nav aria-label="Settings" className="nav">
                  <NavList
                    items={SECONDARY_NAV}
                    current={view}
                    onSelect={setView}
                  />
                </nav>
                {workspace.nativeAvailable && (
                  <button
                    type="button"
                    className="nav-item"
                    aria-label="Setup guide"
                    title="Setup guide"
                    onClick={() => setWelcome(true)}
                  >
                    <Compass size={20} aria-hidden="true" />
                    <span className="nav-label" aria-hidden="true">
                      Setup guide
                    </span>
                  </button>
                )}
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
                  data-status={aiStatus}
                  onClick={() => setView("modelLab")}
                  aria-label={`${aiLabel}. Open Model Lab.`}
                  title={`${aiLabel}. Open Model Lab.`}
                >
                  <span className="status-dot" aria-hidden="true" />
                  <span className="status-text">{aiLabel}</span>
                </button>
              </div>
            </aside>

            <div
              className="main-column"
              // While the reader overlays the list, the list behind it can't be
              // reached (it keeps its scroll position and focus for when the
              // overlay closes), so it's taken out of tab order and the a11y
              // tree rather than removed.
              inert={layout.readerMode === "overlay" ? true : undefined}
            >
              <header className="topbar">
                <nav aria-label="Breadcrumb" className="breadcrumb">
                  <span>{SOURCE_LABELS[workspace.source]}</span>
                  <span aria-hidden="true">/</span>
                  <span aria-current="page">{TITLES[view]}</span>
                </nav>
              </header>

              <main
                id="main"
                ref={mainRef}
                className="main"
                tabIndex={-1}
                onScroll={(event) => {
                  if (view === "home")
                    homeScroll.current = event.currentTarget.scrollTop;
                }}
              >
                {simulatedCode && (
                  <Notice tone="info">
                    Practice mode: this preview simulates “
                    {RECOVERY[simulatedCode].title}” so its message can be
                    checked. Nothing here is a real problem.
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
                      collections={collections}
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
                    <OrganizeView
                      workspace={workspace}
                      organize={organize}
                      collections={collections}
                    />
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
                      onOpenFile={openFromAsk}
                    />
                  )}
                  {view === "activity" && (
                    <ActivityView workspace={workspace} activity={activity} />
                  )}
                  {view === "modelLab" && <ModelLabView />}
                </AnnouncerProvider>
              </main>
            </div>

            {layout.readerMode === "split" && (
              <ResizeHandle
                label="Resize the reader"
                value={layout.readerWidth}
                min={READER_MIN_WIDTH}
                max={readerMaxWidth(appWidth)}
                onChange={resizeReader}
              />
            )}
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
                actions={fileActions(reading).filter(
                  (item) => item.id !== "open",
                )}
                onClose={closeDocument}
                onNavigate={setView}
                isOverlay={layout.readerMode === "overlay"}
              />
            )}
            {collectionDialog && (
              <AddToCollectionDialog
                document={collectionDialog}
                collections={collections}
                onClose={() => setCollectionDialog(null)}
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
            {/* Its own live region: independent of whichever view is showing. */}
            <AnnouncerProvider>
              <FloatingOlioChat
                workspace={workspace}
                relations={relations}
                view={view}
                onNavigate={setView}
                onOpenFile={openFromAsk}
                currentFile={reading}
                localAiLabel={aiLabel}
              />
            </AnnouncerProvider>
          </div>
        </AnnouncerProvider>
      </GenerationReadyProvider>
    </AiIndexProvider>
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
