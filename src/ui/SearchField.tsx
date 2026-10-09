import { Search } from "lucide-react";
import type { Ref } from "react";

interface SearchFieldProps {
  value: string;
  onChange: (value: string) => void;
  label: string;
  placeholder?: string;
  shortcut?: string;
  ref?: Ref<HTMLInputElement>;
}

export function SearchField({
  value,
  onChange,
  label,
  placeholder,
  shortcut,
  ref,
}: SearchFieldProps) {
  return (
    <div className="search-field">
      <Search className="search-field-icon" size={18} aria-hidden="true" />
      <input
        ref={ref}
        type="search"
        aria-label={label}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
      />
      {shortcut && (
        <kbd className="search-field-shortcut" aria-hidden="true">
          {shortcut}
        </kbd>
      )}
    </div>
  );
}
