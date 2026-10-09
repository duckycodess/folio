import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  ModelDescriptor,
  ModelInstallState,
  ModelRole,
  NativeProviderError,
  RuntimeStatus,
} from "../domain/contracts";

export function isAvailable(): boolean {
  return isTauri();
}

export class NativeAdapterError extends Error {
  readonly native: NativeProviderError;

  constructor(native: NativeProviderError) {
    super(native.message);
    this.name = "NativeAdapterError";
    this.native = native;
  }
}

function unavailable(): never {
  throw new NativeAdapterError({
    code: "runtimeMissing",
    message: "Local model adapters are available in the Folio desktop app.",
    detail: "browser-preview",
  });
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isAvailable()) unavailable();
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    if (cause && typeof cause === "object" && "code" in cause && "message" in cause) {
      throw new NativeAdapterError(cause as NativeProviderError);
    }
    throw new NativeAdapterError({
      code: "ioError",
      message: String(cause),
    });
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
