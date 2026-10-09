import { describe, expect, it } from "vitest";
import { folioError } from "../domain/errors";
import {
  actionReducer,
  IDLE,
  type ActionEvent,
  type ActionState,
} from "./actionState";

type Result = { files: number };
const run = (...events: ActionEvent<Result>[]) =>
  events.reduce<ActionState<Result>>(actionReducer, IDLE);
const failure = folioError("workspaceUnavailable", "gone");

describe("action state", () => {
  it("is pending, not successful, while the native call runs", () => {
    expect(run({ type: "start", request: 1 }).status).toBe("pending");
  });

  it("reaches success only from the native success of the pending request", () => {
    const done = run(
      { type: "start", request: 1 },
      { type: "nativeSucceeded", request: 1, result: { files: 3 } },
    );
    expect(done).toEqual({
      status: "succeeded",
      request: 1,
      result: { files: 3 },
    });
  });

  it("never shows success without a request in flight", () => {
    expect(
      run({ type: "nativeSucceeded", request: 1, result: { files: 3 } }).status,
    ).toBe("idle");
  });

  it("ignores a late reply to an older request", () => {
    const state = run(
      { type: "start", request: 1 },
      { type: "start", request: 2 },
      { type: "nativeSucceeded", request: 1, result: { files: 3 } },
    );
    expect(state).toEqual({ status: "pending", request: 2 });
  });

  it("keeps a failure from being replaced by a later success for the same request", () => {
    const state = run(
      { type: "start", request: 1 },
      { type: "nativeFailed", request: 1, error: failure },
      { type: "nativeSucceeded", request: 1, result: { files: 3 } },
    );
    expect(state.status).toBe("failed");
  });

  it("starts over from a new request after a failure", () => {
    const state = run(
      { type: "start", request: 1 },
      { type: "nativeFailed", request: 1, error: failure },
      { type: "start", request: 2 },
      { type: "nativeSucceeded", request: 2, result: { files: 1 } },
    );
    expect(state.status).toBe("succeeded");
  });
});
