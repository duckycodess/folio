import type { ReactNode } from "react";

interface PanelProps {
  title: string;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function Panel({ title, actions, children, className }: PanelProps) {
  return (
    <section
      className={`panel${className ? ` ${className}` : ""}`}
      aria-label={title}
    >
      <div className="panel-header">
        <h2 className="panel-title">{title}</h2>
        {actions && <div className="panel-actions">{actions}</div>}
      </div>
      <div className="panel-body">{children}</div>
    </section>
  );
}
