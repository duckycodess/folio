import { useCallback, useEffect, useRef, useState } from "react";
import {
  forgetOnlineKey,
  isAvailable,
  onlineGenerationStatus,
  saveOnlineKey,
  setOnlineGeneration,
} from "../adapters/onlineGeneration";
import type { OnlineGenerationStatus } from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";

export type OnlineGenerationAction = "saveKey" | "forgetKey" | "toggle";

export interface OnlineGenerationController {
  /** `null` in the browser preview or until the native core answers. */
  status: OnlineGenerationStatus | null;
  busy: OnlineGenerationAction | null;
  error: FolioError | null;
  /** Resolves `true` once the key is checked and saved. */
  saveKey: (key: string) => Promise<boolean>;
  forgetKey: () => void;
  setEnabled: (enabled: boolean, modelId: string, consent: boolean) => void;
  dismiss: () => void;
}

/**
 * Optional online generation for summaries and answers (ADR 0017). Off until
 * the user saves a Groq key and turns it on; every change goes through the
 * native core, which owns the key.
 */
export function useOnlineGeneration(): OnlineGenerationController {
  const [status, setStatus] = useState<OnlineGenerationStatus | null>(null);
  const [busy, setBusy] = useState<OnlineGenerationAction | null>(null);
  const [error, setError] = useState<FolioError | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    if (isAvailable())
      onlineGenerationStatus()
        .then((next) => mounted.current && setStatus(next))
        .catch((cause) => mounted.current && setError(toFolioError(cause)));
    return () => {
      mounted.current = false;
    };
  }, []);

  const run = useCallback(
    async (
      action: OnlineGenerationAction,
      work: () => Promise<OnlineGenerationStatus>,
    ): Promise<boolean> => {
      setBusy(action);
      setError(null);
      try {
        const next = await work();
        if (mounted.current) setStatus(next);
        return true;
      } catch (cause) {
        if (mounted.current) setError(toFolioError(cause));
        return false;
      } finally {
        if (mounted.current) setBusy(null);
      }
    },
    [],
  );

  return {
    status,
    busy,
    error,
    saveKey: (key) => run("saveKey", () => saveOnlineKey(key)),
    forgetKey: () => void run("forgetKey", forgetOnlineKey),
    setEnabled: (enabled, modelId, consent) =>
      void run("toggle", () => setOnlineGeneration(enabled, modelId, consent)),
    dismiss: () => setError(null),
  };
}
