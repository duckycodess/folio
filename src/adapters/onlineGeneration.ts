import { invoke, isTauri } from "@tauri-apps/api/core";
import type { OnlineGenerationStatus } from "../domain/contracts";
import { folioError, toFolioError } from "../domain/errors";

/**
 * Optional online generation through Groq (ADR 0017). The key goes to the
 * native core once, which checks it with Groq and keeps it in the system
 * keychain; it never comes back to the webview.
 */
export function isAvailable(): boolean {
  return isTauri();
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isAvailable())
    throw folioError(
      "modelNotInstalled",
      "Online generation is set up in the Folio desktop app.",
      { component: "runtime", reason: "browserPreview" },
    );
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    throw toFolioError(cause);
  }
}

export function onlineGenerationStatus(): Promise<OnlineGenerationStatus> {
  return call("online_generation_status");
}

export function saveOnlineKey(key: string): Promise<OnlineGenerationStatus> {
  return call("save_online_key", { key });
}

export function forgetOnlineKey(): Promise<OnlineGenerationStatus> {
  return call("forget_online_key");
}

export function setOnlineGeneration(
  enabled: boolean,
  modelId: string,
  consent: boolean,
): Promise<OnlineGenerationStatus> {
  return call("set_online_generation", { enabled, modelId, consent });
}
