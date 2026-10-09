import type { ModelsController } from "./useModels";

export type LocalAiStatus =
  "desktopOnly" | "checking" | "unavailable" | "ready" | "notSetUp";

/**
 * Local AI never reads as ready by assumption: this reads the same
 * `useModels()` state Model Lab shows, and is "ready" only once the writing
 * model Ask & Act actually uses is installed. While that model is still being
 * verified it is "checking", not "not set up".
 */
export function localAiStatus(models: ModelsController): LocalAiStatus {
  switch (models.load) {
    case "desktopOnly":
      return "desktopOnly";
    case "loading":
      return "checking";
    case "failed":
      return "unavailable";
    case "ready": {
      const generationId = models.setup?.selectedGeneration;
      const row = generationId
        ? models.groups
            .flatMap((group) => group.rows)
            .find((candidate) => candidate.descriptor.id === generationId)
        : undefined;
      if (row && !row.state) return "checking";
      return row?.state?.status === "installed" ? "ready" : "notSetUp";
    }
  }
}

const LABELS: Record<LocalAiStatus, string> = {
  desktopOnly: "Local AI needs the desktop app",
  checking: "Checking local AI…",
  unavailable: "Local AI status unavailable",
  ready: "Local AI ready",
  notSetUp: "Local AI not set up",
};

export function localAiStatusLabel(models: ModelsController): string {
  return LABELS[localAiStatus(models)];
}
