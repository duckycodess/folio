# The narrow reader is an overlay, not a full-width replacement

At the 700px minimum and 200% zoom, the reader used to replace the list outright: the list's elements were hidden (`display: none`) and the reader took over the same grid cell as a full-width view with a Back control. That lost the list's scroll position and focus every time, and search, the Ask Olio launcher and its greeting stayed in normal flow above the reader, which looked like floating chrome once the list below them was gone.

The reader is now an overlay instead. It shares the main column's grid cell with the list (`justify-self: end`, its own width, a floating-layer shadow) rather than replacing it, so the list stays mounted behind it — scroll position, focus and all — and `inert` keeps it out of the tab order and the accessibility tree while it's covered. Back and Escape still close it, and focus returns to the row that opened it, same as before. Home's search, filters and other chrome are covered by the overlay rather than sitting above it, since the list they belong to is now the thing being covered.

This also removes the separate "narrow window" breakpoint for this decision. Whether the reader splits beside the list or overlays it now follows the app's own measured width, the sidebar's mode and the reader's own (resizable, remembered) width together — see the "Layout" section of `docs/design.md` and `src/app/shellLayout.ts` — not a fixed 860px window breakpoint. A wide window with a wide reader can still overlay; a narrow one with the reader closed never does.

Decided with Louise on 2026-10-10 for issue #67.
