import { useId, useState } from "react";
import { useOnlineGeneration } from "../app/useOnlineGeneration";
import { Button } from "../ui/Button";
import { Notice } from "../ui/Notice";
import { Panel } from "../ui/Panel";
import { Progress } from "../ui/Progress";

/**
 * Optional online writing through Groq (ADR 0018), off by default. Only
 * summaries and answers use it; the key is checked and kept by the native
 * core and never shown again.
 */
export function OnlineGenerationPanel() {
  const online = useOnlineGeneration();
  const ids = useId();
  const [key, setKey] = useState("");
  const [consent, setConsent] = useState(false);
  const [chosenModel, setChosenModel] = useState<string | null>(null);
  const status = online.status;
  const model = chosenModel ?? status?.modelId ?? "";

  return (
    <Panel title="Online writing (optional)">
      <p className="muted">
        Off unless you turn it on. When it's on, summaries and answers are
        written by Groq, an online service, with your own Groq API key. Search
        and reading your Ask &amp; Act requests stay on this computer, so Ask
        &amp; Act still needs the local writing model above.
      </p>
      <Notice tone="warning">
        While it's on, the passages Folio picks from your files for a summary or
        answer, and your question, are sent to Groq. Whole files and your folder
        list aren't sent. Without internet, summaries and answers stop until you
        turn it off.
      </Notice>
      {online.error && (
        // The native message says what went wrong with this setting itself
        // ("Groq refused the saved key."); the summary-time recovery wording
        // would send the user back to the page they're on.
        <Notice
          tone="danger"
          action={
            <button
              type="button"
              className="link-button"
              onClick={online.dismiss}
            >
              Dismiss
            </button>
          }
        >
          {online.error.message}
        </Notice>
      )}
      {!status ? (
        !online.error && <Progress label="Checking online writing" />
      ) : (
        <>
          {status.keyStored ? (
            <div className="online-key">
              <p>A Groq key is saved in this computer's keychain.</p>
              <Button
                variant="ghost"
                disabled={online.busy !== null}
                onClick={online.forgetKey}
              >
                {online.busy === "forgetKey" ? "Forgetting…" : "Forget key"}
              </Button>
            </div>
          ) : (
            <form
              className="online-key"
              onSubmit={(event) => {
                event.preventDefault();
                void online.saveKey(key).then((saved) => saved && setKey(""));
              }}
            >
              <label htmlFor={`${ids}-key`} className="field-label">
                Groq API key
              </label>
              <input
                id={`${ids}-key`}
                className="text-input"
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={key}
                aria-describedby={`${ids}-key-help`}
                onChange={(event) => setKey(event.target.value)}
              />
              <p id={`${ids}-key-help`} className="field-help">
                Folio checks the key with Groq, then keeps it in this computer's
                keychain. It isn't shown again.
              </p>
              <Button
                type="submit"
                disabled={!key.trim() || online.busy !== null}
              >
                {online.busy === "saveKey" ? "Checking key…" : "Save key"}
              </Button>
            </form>
          )}

          <div className="online-key">
            <label htmlFor={`${ids}-model`} className="field-label">
              Groq model
            </label>
            <select
              id={`${ids}-model`}
              className="text-input"
              value={model}
              disabled={online.busy !== null}
              aria-describedby={`${ids}-model-help`}
              onChange={(event) => {
                setChosenModel(event.target.value);
                if (status.enabled)
                  online.setEnabled(true, event.target.value, true);
              }}
            >
              {status.models.map((choice) => (
                <option key={choice} value={choice}>
                  {choice}
                </option>
              ))}
            </select>
            <p id={`${ids}-model-help`} className="field-help">
              Only models that keep to Folio's output format exactly are listed.
            </p>
          </div>

          {status.enabled ? (
            <div className="online-key">
              <p>
                On: summaries and answers are written online by Groq with{" "}
                {status.modelId}.
              </p>
              <Button
                disabled={online.busy !== null}
                onClick={() => online.setEnabled(false, model, false)}
              >
                Turn off online writing
              </Button>
            </div>
          ) : (
            <div className="online-key">
              <label className="choice">
                <input
                  type="checkbox"
                  checked={consent}
                  onChange={(event) => setConsent(event.target.checked)}
                />
                I understand passages from my files are sent to Groq when I
                summarize or ask.
              </label>
              <Button
                disabled={!status.keyStored || !consent || online.busy !== null}
                onClick={() => online.setEnabled(true, model, consent)}
              >
                Use Groq for summaries and answers
              </Button>
            </div>
          )}
        </>
      )}
    </Panel>
  );
}
