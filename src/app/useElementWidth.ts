import { useLayoutEffect, useRef, useState, type RefObject } from "react";

/**
 * An element's own content-box width, kept current with `ResizeObserver` so
 * layout can follow the space actually available instead of only the
 * window's (#67). `fallback` is used for the one frame before the first
 * measurement, so initial render still has something reasonable to lay out
 * with.
 */
export function useElementWidth<T extends HTMLElement>(
  fallback: number,
): [RefObject<T | null>, number] {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(fallback);

  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    setWidth(element.getBoundingClientRect().width);
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setWidth(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return [ref, width];
}
