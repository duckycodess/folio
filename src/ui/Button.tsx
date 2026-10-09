import type { ButtonHTMLAttributes, ReactNode } from "react";

export type ButtonVariant = "primary" | "secondary" | "ghost";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  icon?: ReactNode;
}

/** One primary button per view; see docs/design.md. */
export function Button({
  variant = "secondary",
  icon,
  className,
  children,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={`button button-${variant}${className ? ` ${className}` : ""}`}
      {...rest}
    >
      {icon && (
        <span className="button-icon" aria-hidden="true">
          {icon}
        </span>
      )}
      {children}
    </button>
  );
}
