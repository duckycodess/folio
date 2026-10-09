import { useEffect, useId, useMemo, useRef, type Ref } from "react";
import type { DocumentRecord } from "../domain/contracts";
import { readerBlocks } from "../domain/pages";

interface ReaderTextProps {
  document: DocumentRecord & { content: string };
  /** The highlighted passage, as string indices into `content`. */
  range: [number, number] | null;
  markRef: Ref<HTMLElement>;
  /** A cited page to show when its passage can't be highlighted. */
  focusPage?: number;
}

/**
 * The read-only text. A PDF is shown page by page under "Page N" headings,
 * which screen readers can jump between, with pages whose text couldn't be
 * extracted listed in place. TXT and Markdown are one block.
 */
export function ReaderText({
  document,
  range,
  markRef,
  focusPage,
}: ReaderTextProps) {
  const { content } = document;
  const blocks = useMemo(
    () => readerBlocks(content, document.pages, document.unreadablePages),
    [content, document.pages, document.unreadablePages],
  );
  const pagesRef = useRef<HTMLDivElement>(null);
  const id = useId();

  // A stale passage can't be highlighted, but its page can still be shown.
  useEffect(() => {
    if (range || focusPage === undefined) return;
    pagesRef.current
      ?.querySelector(`[data-page="${focusPage}"]`)
      ?.scrollIntoView({ block: "start" });
  }, [range, focusPage]);

  if (!blocks)
    return (
      <pre className="source-text">
        {highlighted(content, 0, content.length, range, markRef)}
      </pre>
    );

  return (
    <div className="reader-pages" ref={pagesRef}>
      {blocks.map((block) => {
        const headingId = `${id}-page-${block.page}`;
        const text =
          block.kind === "page" ? content.slice(block.start, block.end) : "";
        return (
          <section
            key={`${block.kind}-${block.page}`}
            className="reader-page"
            data-page={block.page}
            aria-labelledby={headingId}
          >
            <h4 id={headingId} className="page-label">
              Page {block.page}
            </h4>
            {block.kind === "unreadable" ? (
              <p className="muted">Page {block.page} couldn't be read.</p>
            ) : text.trim() ? (
              <pre className="source-text">
                {highlighted(content, block.start, block.end, range, markRef)}
              </pre>
            ) : (
              <p className="muted">No text on this page.</p>
            )}
          </section>
        );
      })}
    </div>
  );
}

/** `content[start, end)`, with the part inside `range` marked. */
function highlighted(
  content: string,
  start: number,
  end: number,
  range: [number, number] | null,
  markRef: Ref<HTMLElement>,
) {
  if (!range || range[1] <= start || range[0] >= end)
    return content.slice(start, end);
  const from = Math.max(range[0], start);
  const to = Math.min(range[1], end);
  return (
    <>
      {content.slice(start, from)}
      <mark ref={markRef} className="source-highlight" tabIndex={-1}>
        {content.slice(from, to)}
      </mark>
      {content.slice(to, end)}
    </>
  );
}
