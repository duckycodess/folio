import { Sparkles, X } from "lucide-react";
import { useState } from "react";

const GREETING_KEY = "folio.askOlio.greetingDismissed";

// Storage can be missing or throw; the greeting then shows again next time.
function greetingDismissed(): boolean {
  try {
    return window.localStorage.getItem(GREETING_KEY) === "1";
  } catch {
    return false;
  }
}

function rememberDismissed() {
  try {
    window.localStorage.setItem(GREETING_KEY, "1");
  } catch {
    // Dismissed for this session only.
  }
}

/**
 * Home's way into Ask & Act: a labelled button, never an auto-opening chat.
 * It opens Ask & Act with Home's search and folder filled in, and sends
 * nothing; the greeting, once dismissed, stays dismissed on this device.
 * No Olio image here: Home's header already has the one Olio per view.
 */
export function AskOlioLauncher({
  query,
  folder,
  onOpen,
}: {
  query: string;
  /** Home's folder filter, which becomes Ask & Act's scope. */
  folder: string | null;
  onOpen: () => void;
}) {
  const [greeting, setGreeting] = useState(() => !greetingDismissed());
  const carried = [
    query.trim() && `“${query.trim()}”`,
    folder && `in ${folder}`,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className="ask-launcher">
      {greeting && (
        <p className="ask-launcher-greeting">
          Hello! Need a deeper search?
          <button
            type="button"
            className="icon-button ask-launcher-dismiss"
            aria-label="Dismiss greeting"
            onClick={() => {
              rememberDismissed();
              setGreeting(false);
            }}
          >
            <X size={16} aria-hidden="true" />
          </button>
        </p>
      )}
      <button
        type="button"
        className="button button-secondary ask-launcher-button"
        aria-describedby="ask-launcher-help"
        onClick={onOpen}
      >
        <Sparkles size={18} aria-hidden="true" />
        Ask Olio
      </button>
      <span id="ask-launcher-help" className="visually-hidden">
        Opens Ask &amp; Act
        {carried ? ` with ${carried} filled in` : ""}. Nothing is sent until you
        ask.
      </span>
    </div>
  );
}
