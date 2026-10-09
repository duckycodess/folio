import type { AskOutcome } from "../app/askAct";
import { failSoon, simulatedFailure } from "./simulate";

export const PRACTICE_LABEL = "Practice replies — not a model";

const WORDS_PER_FRAME = 3;
const FRAME_MS = 120;

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function fabricate(request: string): string {
  const topic = request.trim() || "your files";
  return (
    `This is a practice reply, not a real model. A local writing model ` +
    `would answer “${topic}” from your files, with citations you can open. ` +
    `Set one up in Model Lab to try a real reply.`
  );
}

/**
 * Stands in for the real AI adapters only in the browser preview — every
 * call site checks `isAvailable()` first and never reaches this in the
 * desktop app. Lets the compact chat's loading, streaming, error and Retry
 * states be checked without a local model. `onProgress` reports each
 * streamed frame; `?simulate=<code>` (#9's existing practice mode) can still
 * fail the request, so Retry can be checked too.
 */
export async function mockReply(
  request: string,
  onProgress: (partial: AskOutcome) => void,
): Promise<AskOutcome> {
  const simulated = simulatedFailure("assistant");
  if (simulated) await failSoon(simulated);
  await wait(400);
  const full = fabricate(request);
  const words = full.split(" ");
  for (
    let shown = WORDS_PER_FRAME;
    shown < words.length;
    shown += WORDS_PER_FRAME
  ) {
    onProgress({
      type: "practice",
      reply: words.slice(0, shown).join(" "),
      streaming: true,
    });
    await wait(FRAME_MS);
  }
  return { type: "practice", reply: full, streaming: false };
}
