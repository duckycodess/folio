import type { ReactNode } from "react";

interface ListRowProps {
  icon?: ReactNode;
  title: string;
  subtitle?: string;
  meta?: ReactNode;
  selected?: boolean;
  onSelect: () => void;
}

/** A selectable row inside an element with `role="listbox"`. */
export function ListRow({
  icon,
  title,
  subtitle,
  meta,
  selected = false,
  onSelect,
}: ListRowProps) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      className={`list-row${selected ? " is-selected" : ""}`}
      onClick={onSelect}
      title={subtitle ? `${title}\n${subtitle}` : title}
    >
      {icon && (
        <span className="list-row-icon" aria-hidden="true">
          {icon}
        </span>
      )}
      <span className="list-row-text">
        <span className="list-row-title">{title}</span>
        {subtitle && <span className="list-row-subtitle">{subtitle}</span>}
      </span>
      {meta && <span className="list-row-meta">{meta}</span>}
    </button>
  );
}
