import { describe, expect, it } from "vitest";
import type { ModelDescriptor } from "../domain/contracts";
import { localAiStatus, localAiStatusLabel } from "./localAi";
import type { ModelGroup } from "./models";
import type { ModelsController } from "./useModels";

const noop = () => undefined;

function controller(partial: Partial<ModelsController>): ModelsController {
  return {
    load: "ready",
    groups: [],
    setup: null,
    runtime: null,
    installing: null,
    saving: null,
    error: null,
    notice: null,
    install: noop,
    cancel: noop,
    remove: noop,
    select: noop,
    reload: noop,
    dismiss: noop,
    ...partial,
  };
}

function groupWith(
  id: string,
  status: "installed" | "notInstalled" | undefined,
): ModelGroup {
  return {
    role: "generation",
    rows: [
      {
        descriptor: { id, role: "generation" } as ModelDescriptor,
        state: status && { id, status },
        selected: true,
        downloadBytes: 0,
      },
    ],
  };
}

describe("localAiStatusLabel", () => {
  it("never claims readiness outside the desktop app", () => {
    expect(localAiStatusLabel(controller({ load: "desktopOnly" }))).toBe(
      "Local AI needs the desktop app",
    );
  });

  it("says it's checking while models are loading", () => {
    expect(localAiStatusLabel(controller({ load: "loading" }))).toBe(
      "Checking local AI…",
    );
  });

  it("says status is unavailable rather than guessing after a load failure", () => {
    expect(localAiStatusLabel(controller({ load: "failed" }))).toBe(
      "Local AI status unavailable",
    );
  });

  it("is not set up when no writing model is selected", () => {
    expect(localAiStatusLabel(controller({ load: "ready", setup: null }))).toBe(
      "Local AI not set up",
    );
  });

  it("is not set up when the selected writing model isn't installed yet", () => {
    const groups = [groupWith("writer", "notInstalled")];
    expect(
      localAiStatusLabel(
        controller({
          load: "ready",
          groups,
          setup: {
            selectedEmbedding: null,
            selectedGeneration: "writer",
            hostRuntimeId: "cpu",
            hostRuntimeBytes: null,
            deviceMemoryBytes: null,
            availableDiskBytes: null,
          },
        }),
      ),
    ).toBe("Local AI not set up");
  });

  it("is ready only once the real selected writing model is installed", () => {
    const groups = [groupWith("writer", "installed")];
    expect(
      localAiStatusLabel(
        controller({
          load: "ready",
          groups,
          setup: {
            selectedEmbedding: null,
            selectedGeneration: "writer",
            hostRuntimeId: "cpu",
            hostRuntimeBytes: null,
            deviceMemoryBytes: null,
            availableDiskBytes: null,
          },
        }),
      ),
    ).toBe("Local AI ready");
  });

  it("is still checking while the selected writing model is being verified", () => {
    const groups = [groupWith("writer", undefined)];
    const models = controller({
      load: "ready",
      groups,
      setup: {
        selectedEmbedding: null,
        selectedGeneration: "writer",
        hostRuntimeId: "cpu",
        hostRuntimeBytes: null,
        deviceMemoryBytes: null,
        availableDiskBytes: null,
      },
    });
    expect(localAiStatus(models)).toBe("checking");
    expect(localAiStatusLabel(models)).toBe("Checking local AI…");
  });
});
