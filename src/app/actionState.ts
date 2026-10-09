import type { FolioError } from "../domain/errors";

/**
 * The state of one user action that the native core carries out (opening a
 * folder now; saving and undo later). Success is a state the UI can reach
 * only from the native core's own success, and only for the latest request:
 * a slow reply to an older request can't overwrite a newer one.
 */
export type ActionState<T> =
  | { status: "idle" }
  | { status: "pending"; request: number }
  | { status: "succeeded"; request: number; result: T }
  | { status: "failed"; request: number; error: FolioError };

export type ActionEvent<T> =
  | { type: "start"; request: number }
  | { type: "nativeSucceeded"; request: number; result: T }
  | { type: "nativeFailed"; request: number; error: FolioError }
  | { type: "reset" };

export const IDLE: ActionState<never> = { status: "idle" };

export function actionReducer<T>(
  state: ActionState<T>,
  event: ActionEvent<T>,
): ActionState<T> {
  switch (event.type) {
    case "start":
      return { status: "pending", request: event.request };
    case "reset":
      return IDLE;
    case "nativeSucceeded":
    case "nativeFailed": {
      // Only the reply to the request still pending may settle it.
      if (state.status !== "pending" || state.request !== event.request)
        return state;
      return event.type === "nativeSucceeded"
        ? { status: "succeeded", request: event.request, result: event.result }
        : { status: "failed", request: event.request, error: event.error };
    }
  }
}
