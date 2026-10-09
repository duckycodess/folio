import { RECOVERY, type RecoveryFlow } from "../app/recovery";
import type { FolioErrorCode } from "../domain/contracts";
import {
  folioError,
  isFolioErrorCode,
  type FolioError,
} from "../domain/errors";
import { nativeAvailable } from "./workspace";

/**
 * Browser-preview practice mode, for checking error states (#9). Open the
 * preview with `?simulate=<code>` and the flow that code belongs to fails with
 * it. Never active in the desktop app.
 */
export function simulationFrom(
  search: string,
  native: boolean,
): FolioErrorCode | undefined {
  if (native) return undefined;
  const code = new URLSearchParams(search).get("simulate");
  return isFolioErrorCode(code) ? code : undefined;
}

export const simulatedCode = simulationFrom(
  typeof location === "undefined" ? "" : location.search,
  nativeAvailable,
);

/** The simulated failure for `flow`, if practice mode targets it. */
export function simulatedFailure(flow: RecoveryFlow): FolioError | undefined {
  if (!simulatedCode || RECOVERY[simulatedCode].flow !== flow) return undefined;
  return folioError(simulatedCode, `Simulated for the browser preview.`);
}

/** Simulated failures arrive a moment later, like real ones. */
export function failSoon(error: FolioError): Promise<never> {
  return new Promise((_, reject) => setTimeout(() => reject(error), 300));
}
