import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  ModelDescriptor,
  ModelInstallState,
  ModelRole,
  RuntimeStatus,
} from "../domain/contracts";
import { folioError, toFolioError } from "../domain/errors";

export function isAvailable(): boolean {
  return isTauri();
}

function unavailable(): never {
  throw folioError(
    "modelNotInstalled",
    "Local model adapters are available in the Folio desktop app.",
    { component: "runtime", reason: "browserPreview" },
  );
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isAvailable()) unavailable();
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    throw toFolioError(cause);
  }
}

export function listModels(): Promise<ModelDescriptor[]> {
  return call("list_models");
}

export function verifyModel(modelId: string): Promise<ModelInstallState> {
  return call("verify_model", { modelId });
}

export function installModel(modelId: string): Promise<ModelInstallState> {
  return call("install_model", { modelId });
}

export function removeModel(modelId: string): Promise<void> {
  return call("remove_model", { modelId });
}

export function selectModel(role: ModelRole, modelId: string): Promise<void> {
  return call("select_model", { role, modelId });
}

export function runtimeStatus(runtimeId: string): Promise<RuntimeStatus> {
  return call("runtime_status", { runtimeId });
}

export function installRuntime(runtimeId: string): Promise<RuntimeStatus> {
  return call("install_runtime", { runtimeId });
}
