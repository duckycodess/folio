import { createContext, useContext } from "react";

/**
 * Whether a local writing model is set up and ready, read once by the shell's
 * `useModels`. Views read it here instead of starting their own `useModels`,
 * whose first load re-verifies every installed model file (a full SHA-256 of
 * GBs of model weights) and reports "not ready" until that finishes.
 */
const GenerationReadyContext = createContext(false);
export const GenerationReadyProvider = GenerationReadyContext.Provider;

/** False outside the shell (tests, previews): nothing is offered there. */
export function useGenerationReady(): boolean {
  return useContext(GenerationReadyContext);
}
