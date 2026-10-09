import {
  createContext,
  useCallback,
  useContext,
  useState,
  type ReactNode,
} from "react";

const AnnounceContext = createContext<(message: string) => void>(() => {});

/**
 * One polite live region that is always in the page. Screen readers often skip
 * a live region that is inserted with its text already in it, so notices write
 * their text here instead.
 */
export function AnnouncerProvider({ children }: { children: ReactNode }) {
  const [message, setMessage] = useState("");
  const announce = useCallback((text: string) => {
    // Clear first so the same text is announced again when it repeats.
    setMessage("");
    requestAnimationFrame(() => setMessage(text));
  }, []);

  return (
    <AnnounceContext.Provider value={announce}>
      {children}
      <div className="visually-hidden" role="status">
        {message}
      </div>
    </AnnounceContext.Provider>
  );
}

export function useAnnounce() {
  return useContext(AnnounceContext);
}
