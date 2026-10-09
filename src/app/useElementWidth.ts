import { useLayoutEffect, useState } from "react";

/**
 * An element's own content-box width, kept current with `ResizeObserver` so
 * layout can follow the space actually available instead of only the
 * window's (#67). `fallback` is used until the element is first measured, so
 * initial render still has something reasonable to lay out with.
 *
 * The returned ref is a callback, so the observer follows the element even
 * when it mounts later or is replaced: the shell renders Welcome first, and
 * reopening the setup guide unmounts the app container.
 */
export function useElementWidth<T extends HTMLElement>(
  fallback: number,
): [(element: T | null) => void, number, T | null] {
  const [element, setElement] = useState<T | null>(null);
  const [width, setWidth] = useState(fallback);

  useLayoutEffect(() => {
    if (!element) return;
    setWidth(element.getBoundingClientRect().width);
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setWidth(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [element]);

  return [setElement, width, element];
}
