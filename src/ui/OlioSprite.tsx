import { olioSource, type OlioPose, type OlioSize } from "./Olio";

export const OLIO_SPRITE_STATES = [
  "idle",
  "blink",
  "wave",
  "thinking",
  "responding",
] as const;

export type OlioSpriteState = (typeof OLIO_SPRITE_STATES)[number];

// Placeholder art: built from the twelve static poses already in
// src/assets/olio/, animated with CSS. Final mascot artwork (#66) swaps the
// implementation of this one component; `<OlioSprite state size>` stays the
// same for every caller.
const POSE_FOR_STATE: Record<OlioSpriteState, OlioPose> = {
  idle: "default",
  blink: "default",
  wave: "waving",
  thinking: "thinking",
  responding: "success",
};

/**
 * The floating launcher's mascot. Always decorative (the adjacent label
 * carries the meaning). `prefers-reduced-motion` freezes it on the resting
 * pose for its state (see src/styles/chat.css).
 */
export function OlioSprite({
  state,
  size,
}: {
  state: OlioSpriteState;
  size: OlioSize;
}) {
  const { src, srcSet } = olioSource(POSE_FOR_STATE[state], size);
  return (
    <span className={`olio-sprite olio-sprite-${state}`} aria-hidden="true">
      <img
        src={src}
        srcSet={srcSet}
        width={size}
        height={size}
        alt=""
        draggable={false}
      />
    </span>
  );
}
