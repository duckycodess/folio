import type {
  DocumentRecord,
  GroundedResult,
  SourcePassage,
} from "../domain/contracts";

const EXCERPT_LENGTH = 120;

function excerpt(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > EXCERPT_LENGTH
    ? `${flat.slice(0, EXCERPT_LENGTH - 1)}…`
    : flat;
}

/**
 * Generated sentences, each with numbered sources that open the exact
 * passage. A sentence without a source says so, and the count of such
 * sentences follows the list.
 */
export function CitedSentences({
  result,
  byId,
  onOpen,
}: {
  result: Pick<GroundedResult, "sentences" | "uncitedSentenceCount">;
  /** Names the file of each source, for answers that cite several files. */
  byId?: Map<string, DocumentRecord>;
  onOpen: (passage: SourcePassage) => void;
}) {
  const sources: string[] = [];
  const numberOf = (passage: SourcePassage) => {
    const key = `${passage.documentId}|${passage.start}|${passage.end}`;
    if (!sources.includes(key)) sources.push(key);
    return sources.indexOf(key) + 1;
  };
  const uncited = result.uncitedSentenceCount;
  return (
    <>
      <ol className="summary-points">
        {result.sentences.map((sentence, index) => (
          <li key={index}>
            {sentence.text}{" "}
            {sentence.citations.length ? (
              sentence.citations.map((citation) => {
                const n = numberOf(citation);
                const file = byId?.get(citation.documentId)?.name;
                const where = [
                  file && `in ${file}`,
                  citation.page !== undefined && `page ${citation.page}`,
                ]
                  .filter(Boolean)
                  .join(", ");
                return (
                  <button
                    key={`${citation.documentId}-${citation.start}-${citation.end}`}
                    type="button"
                    className="citation"
                    aria-label={`Source ${n}${where ? `, ${where}` : ""}: ${excerpt(citation.text)}`}
                    title={`${file ? `${file}: ` : ""}${excerpt(citation.text)}`}
                    onClick={() => onOpen(citation)}
                  >
                    {n}
                  </button>
                );
              })
            ) : (
              <span className="muted">(no source)</span>
            )}
          </li>
        ))}
      </ol>
      {uncited > 0 && (
        <p className="muted">
          {uncited}{" "}
          {uncited === 1 ? "point has no source" : "points have no source"}.
          Check {uncited === 1 ? "it" : "them"} against the files.
        </p>
      )}
    </>
  );
}
