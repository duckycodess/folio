import { DatabaseZap, Quote } from "lucide-react";
import type { WorkspaceState } from "../app/useWorkspace";
import type { SearchResult, SourcePassage } from "../domain/contracts";
import { highlightSegments, matchLabel } from "../domain/searchEvidence";
import type { ViewId } from "../shell/navigation";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";

/** Excerpts shown per result; the reader has the rest. */
const PASSAGES_SHOWN = 2;

/** Why one file matched: how, and the passages, each opening the reader there. */
export function ResultEvidence({
  result,
  query,
  onOpenPassage,
}: {
  result: SearchResult;
  query: string;
  onOpenPassage: (passage: SourcePassage) => void;
}) {
  const passages = result.passages.slice(0, PASSAGES_SHOWN);
  return (
    <div className="result-evidence">
      <Badge>{matchLabel(result)}</Badge>
      {/* Keeps the spoken description from running the label into the excerpt. */}
      <span className="visually-hidden">. </span>
      {passages.map((passage) => (
        <button
          key={`${passage.start}-${passage.end}`}
          type="button"
          className="evidence-excerpt"
          onClick={() => onOpenPassage(passage)}
        >
          {/* A hidden prefix rather than an `aria-label`: the spoken name has
              to contain the excerpt as it is shown, so that naming the control
              out loud still works. */}
          <span className="visually-hidden">
            Open {result.document.name} at this passage:{" "}
          </span>
          <Quote size={14} aria-hidden="true" />
          <span className="evidence-text">
            {passage.page !== undefined && (
              <span className="evidence-page">Page {passage.page} · </span>
            )}
            {highlightSegments(
              excerptText(passage, result.document.sizeBytes),
              query,
            ).map((segment, index) =>
              segment.hit ? (
                <mark key={index}>{segment.text}</mark>
              ) : (
                <span key={index}>{segment.text}</span>
              ),
            )}
          </span>
        </button>
      ))}
      {result.passages.length > PASSAGES_SHOWN && (
        <span className="muted evidence-more">
          {result.passages.length - PASSAGES_SHOWN} more in this file
        </span>
      )}
    </div>
  );
}

/**
 * The excerpt on one line. Where it starts or ends partway through the file,
 * the cut-off word is dropped and "…" shown instead; the reader has the exact
 * passage.
 */
function excerptText(passage: SourcePassage, sizeBytes: number): string {
  let flat = passage.text.replace(/\s+/g, " ").trim();
  if (passage.start > 0) {
    const space = flat.indexOf(" ");
    flat = `…${space > 0 && space < 24 ? flat.slice(space) : ` ${flat}`}`;
  }
  if (passage.end < sizeBytes) {
    const space = flat.lastIndexOf(" ");
    flat = `${space > flat.length - 24 ? flat.slice(0, space) : flat} …`;
  }
  return flat;
}

const PHASES: Record<string, string> = {
  discovering: "Finding files",
  indexing: "Reading files",
  linking: "Finding links between files",
  done: "Finishing",
  cancelled: "Stopping",
};

/**
 * Says what search covers in an open folder, and offers to index it. Shown
 * only while searching, so Home stays calm otherwise.
 */
export function FolderSearchStatus({
  workspace,
  onNavigate,
}: {
  workspace: WorkspaceState;
  onNavigate: (view: ViewId) => void;
}) {
  const search = workspace.search;
  if (search.searchFailure)
    return (
      <RecoveryNotice
        error={search.searchFailure}
        actions={{
          retry: search.retrySearch,
          openModelLab: () => onNavigate("modelLab"),
        }}
      />
    );
  switch (search.index) {
    case "missing":
      return (
        <div className="search-coverage">
          <DatabaseZap size={18} aria-hidden="true" />
          <p>
            Only file names are searched until Folio indexes this folder.
            Indexing reads the files here; it doesn't change them.
          </p>
          <Button onClick={search.buildIndex}>Index this folder</Button>
        </div>
      );
    case "indexing": {
      const progress = search.progress;
      const value =
        progress && progress.total > 0
          ? (100 * progress.processed) / progress.total
          : undefined;
      return (
        <div className="search-coverage">
          <div className="search-coverage-progress">
            <Progress
              label={progress ? PHASES[progress.phase] : "Indexing this folder"}
              value={value}
            />
          </div>
          <Button variant="ghost" onClick={search.cancelIndex}>
            Stop
          </Button>
        </div>
      );
    }
    case "failed":
      return search.indexFailure ? (
        <RecoveryNotice
          error={search.indexFailure}
          actions={{ retry: search.buildIndex }}
        />
      ) : null;
    default:
      return null;
  }
}
