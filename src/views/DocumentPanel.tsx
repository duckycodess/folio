import { ArrowLeft, CornerUpLeft, Sparkles, X } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import { highlightRange, passageState } from "../domain/connections";
import { ReaderText } from "./ReaderText";
import type { DocumentRecord } from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { Progress } from "../ui/Progress";
import { RowMenu, type RowMenuItem } from "../ui/RowMenu";
import { RelatedList } from "./Connections";
import { fileKind, formatBytes, formatModified, languageLabel } from "./format";

const TABS = ["Summary", "Details", "Related"] as const;
type Tab = (typeof TABS)[number];

interface DocumentPanelProps {
  document: DocumentRecord;
  workspace: WorkspaceState;
  relations: RelationshipsState;
  /** Opens on this tab, e.g. after "Show related" from a file row. */
  initialTab?: Tab;
  /** The file's actions (Rename, Move…), the same as its row's ⋯ menu. */
  actions?: RowMenuItem[];
  onClose: () => void;
  onNavigate: (view: ViewId) => void;
}

export function DocumentPanel({
  document,
  workspace,
  relations,
  initialTab,
  actions,
  onClose,
  onNavigate,
}: DocumentPanelProps) {
  // Evidence opened from Related or Graph shows the passage in Details.
  const focus =
    relations.focus?.documentId === document.id ? relations.focus : null;
  const [tab, setTab] = useState<Tab>(
    relations.returnedTo === document.id
      ? "Related"
      : (initialTab ?? "Details"),
  );
  const mark = useRef<HTMLElement>(null);
  const origin = relations.trail[relations.trail.length - 1];
  const range =
    focus && document.content !== undefined
      ? highlightRange(focus, document)
      : null;

  useEffect(() => {
    if (!focus) return;
    setTab("Details");
  }, [focus]);

  // Once the passage is on screen, bring it into view and move focus to it.
  useEffect(() => {
    if (!range || tab !== "Details") return;
    mark.current?.scrollIntoView({ block: "center" });
    mark.current?.focus({ preventScroll: true });
  }, [range?.[0], range?.[1], tab]);
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const id = useId();
  const heading = useRef<HTMLHeadingElement>(null);

  // The panel is keyed by document, so this runs each time one opens. In
  // narrow windows the panel replaces the list, so the row that opened it is
  // hidden; elsewhere focus stays on the list unless it was lost (a Related
  // link replaced the previous panel).
  useEffect(() => {
    const active = window.document.activeElement;
    if (
      window.matchMedia("(max-width: 860px)").matches ||
      !active ||
      active === window.document.body
    )
      heading.current?.focus();
  }, []);

  function onTabKeyDown(event: KeyboardEvent, index: number) {
    const next =
      event.key === "ArrowRight"
        ? (index + 1) % TABS.length
        : event.key === "ArrowLeft"
          ? (index - 1 + TABS.length) % TABS.length
          : null;
    if (next === null) return;
    event.preventDefault();
    setTab(TABS[next]);
    tabRefs.current[next]?.focus();
  }

  return (
    <aside className="document-panel" aria-label={`${document.name} details`}>
      <div className="document-panel-header">
        <button
          type="button"
          className="icon-button document-back"
          aria-label="Back to files"
          onClick={onClose}
        >
          <ArrowLeft size={18} aria-hidden="true" />
        </button>
        <FileTypeIcon mediaType={document.mediaType} />
        <div className="document-heading">
          <h2
            ref={heading}
            className="document-title"
            title={document.name}
            tabIndex={-1}
          >
            {document.name}
          </h2>
          <p className="document-meta tabular">
            {fileKind(document)} · {formatBytes(document.sizeBytes)}
            {document.modifiedAtMs !== undefined &&
              ` · ${formatModified(document.modifiedAtMs)}`}
          </p>
        </div>
        {actions && actions.length > 0 && (
          <RowMenu
            label={`Actions for ${document.name}`}
            items={actions}
            tabbable
          />
        )}
        <button
          type="button"
          className="icon-button document-close"
          aria-label="Close details"
          onClick={onClose}
        >
          <X size={18} aria-hidden="true" />
        </button>
      </div>

      {origin && (
        <button
          type="button"
          className="link-button trail-back"
          onClick={relations.back}
        >
          <CornerUpLeft size={16} aria-hidden="true" />
          Back to {origin.name}
        </button>
      )}

      <div role="tablist" aria-label="Document" className="tabs">
        {TABS.map((name, index) => (
          <button
            key={name}
            ref={(element) => {
              tabRefs.current[index] = element;
            }}
            type="button"
            role="tab"
            id={`${id}-tab-${name}`}
            // Only the selected tab's panel is rendered.
            aria-controls={tab === name ? `${id}-panel-${name}` : undefined}
            aria-selected={tab === name}
            tabIndex={tab === name ? 0 : -1}
            className={`tab${tab === name ? " is-active" : ""}`}
            onClick={() => setTab(name)}
            onKeyDown={(event) => onTabKeyDown(event, index)}
          >
            {name}
          </button>
        ))}
      </div>

      <div
        role="tabpanel"
        id={`${id}-panel-${tab}`}
        aria-labelledby={`${id}-tab-${tab}`}
        className="tab-panel"
        tabIndex={0}
      >
        {tab === "Summary" && (
          <EmptyState
            icon={<Sparkles size={24} />}
            title="Summaries need a local AI model"
            action={
              <Button
                variant="secondary"
                onClick={() => onNavigate("modelLab")}
              >
                Open Model Lab
              </Button>
            }
          >
            You can still read the whole file in Details.
          </EmptyState>
        )}

        {tab === "Details" && (
          <>
            <dl className="details-list">
              {/* The full path wraps here; lists truncate it. */}
              <dt>Path</dt>
              <dd>{document.relativePath}</dd>
              <dt>{workspace.workspace ? "In folder" : "Source"}</dt>
              <dd>
                {workspace.workspace
                  ? workspace.workspace.rootPath
                  : "Sample file bundled with Folio"}
              </dd>
              <dt>Type</dt>
              <dd>{fileKind(document)}</dd>
              <dt>Language</dt>
              <dd>{languageLabel(document.language)}</dd>
              <dt>Size</dt>
              <dd className="tabular">{formatBytes(document.sizeBytes)}</dd>
              <dt>Modified</dt>
              <dd className="tabular">
                {document.modifiedAtMs === undefined
                  ? "Not recorded"
                  : formatModified(document.modifiedAtMs)}
              </dd>
            </dl>
            <div className="subsection-header">
              <h3 className="subsection-title">Contents</h3>
              <Badge>Read-only</Badge>
            </div>
            {workspace.busy && document.content === undefined ? (
              <Progress label="Reading file" />
            ) : document.content !== undefined ? (
              <>
                {focus && !range && (
                  <p className="muted">
                    {passageState(focus, document) === "changed"
                      ? "This file has changed since the connection was found, so the passage can't be highlighted."
                      : "The passage can't be shown in this file."}
                  </p>
                )}
                <ReaderText
                  document={{ ...document, content: document.content }}
                  range={range}
                  markRef={mark}
                  focusPage={focus?.page}
                />
              </>
            ) : (
              <p className="muted">
                {document.mediaType === "application/pdf"
                  ? workspace.nativeAvailable
                    ? "Folio couldn't read this PDF's text."
                    : "PDF text is read in the desktop app; this preview can't read PDFs."
                  : "This file hasn't been read yet."}
              </p>
            )}
          </>
        )}

        {tab === "Related" && (
          <RelatedList
            document={document}
            workspace={workspace}
            relations={relations}
          />
        )}
      </div>
    </aside>
  );
}
