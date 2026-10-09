import { useRef, type KeyboardEvent, type PointerEvent } from "react";

interface ResizeHandleProps {
  /** Current reader width in pixels. */
  value: number;
  min: number;
  max: number;
  /** One press of Left/Right; Home and End jump straight to min/max. */
  step?: number;
  label: string;
  onChange: (next: number) => void;
}

/**
 * The vertical separator between the file list and the reader. Dragging it
 * (or Left/Right, Home/End with the keyboard) resizes the reader; the width
 * is owned by the caller so it can be clamped and remembered (#67).
 */
export function ResizeHandle({
  value,
  min,
  max,
  step = 16,
  label,
  onChange,
}: ResizeHandleProps) {
  const dragStart = useRef<{ pointerX: number; width: number } | null>(null);

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    event.currentTarget.setPointerCapture(event.pointerId);
    dragStart.current = { pointerX: event.clientX, width: value };
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const start = dragStart.current;
    if (!start) return;
    // The reader sits to the right of the separator: dragging left widens it.
    onChange(start.width - (event.clientX - start.pointerX));
  }

  function onPointerUp(event: PointerEvent<HTMLDivElement>) {
    if (event.currentTarget.hasPointerCapture(event.pointerId))
      event.currentTarget.releasePointerCapture(event.pointerId);
    dragStart.current = null;
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    switch (event.key) {
      case "ArrowLeft":
        onChange(value + step);
        break;
      case "ArrowRight":
        onChange(value - step);
        break;
      case "Home":
        onChange(min);
        break;
      case "End":
        onChange(max);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  return (
    <div
      className="resize-handle"
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={Math.round(value)}
      aria-valuemin={min}
      aria-valuemax={max}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onKeyDown={onKeyDown}
    />
  );
}
