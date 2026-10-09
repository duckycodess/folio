import type { ModelsController } from "./useModels";

/**
 * The floating chat's header never assumes local AI is ready: it reads the
 * same `useModels()` state Model Lab shows, and only calls it "ready" once
 * the writing model Ask & Act actually uses is installed.
 */
export function localAiStatusLabel(models: ModelsController): string {
  switch (models.load) {
    case "desktopOnly":
      return "Local AI needs the desktop app";
    case "loading":
      return "Checking local AI…";
    case "failed":
      return "Local AI status unavailable";
    case "ready": {
      const generationId = models.setup?.selectedGeneration;
      const row = generationId
        ? models.groups
            .flatMap((group) => group.rows)
            .find((candidate) => candidate.descriptor.id === generationId)
        : undefined;
      return row?.state?.status === "installed"
        ? "Local AI ready"
        : "Local AI not set up";
    }
  }
}
