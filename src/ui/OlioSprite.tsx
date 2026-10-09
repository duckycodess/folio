import { useSyncExternalStore } from "react";
import olioAnimated from "../assets/olio/olio-animated.webp";
import olioPoster from "../assets/olio/olio-animated-poster.png";

export const OLIO_SPRITE_STATES = ["idle", "thinking"] as const;

export type OlioSpriteState = (typeof OLIO_SPRITE_STATES)[number];

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

// A CSS `animation` pauses under prefers-reduced-motion, but a WebP's own
// frame playback does not: only swapping to a still image actually stops it.
function subscribe(onChange: () => void) {
  const query = window.matchMedia(REDUCED_MOTION_QUERY);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}
function snapshot() {
  return window.matchMedia(REDUCED_MOTION_QUERY).matches;
}
function useReducedMotion() {
  return useSyncExternalStore(subscribe, snapshot, () => false);
}

/**
 * The floating launcher's mascot: the brand's one continuous animated loop
 * (idle, blink and wave are baked into its frames, not separate poses — see
 * docs/design.md "Assets"). `state="thinking"` layers a CSS breathing effect
 * on top of the same loop rather than switching art, since the loop has no
 * separate state frames. Always decorative (the adjacent label carries the
 * meaning). `prefers-reduced-motion` swaps in a still frame and drops the
 * breathing effect (see src/styles/chat.css).
 */
export function OlioSprite({
  state,
  size,
}: {
  state: OlioSpriteState;
  size: number;
}) {
  const reducedMotion = useReducedMotion();
  return (
    <span
      className={`olio-sprite olio-sprite-${state}`}
      style={{ width: size, height: size }}
      aria-hidden="true"
    >
      <img
        src={reducedMotion ? olioPoster : olioAnimated}
        width={size}
        height={size}
        alt=""
        draggable={false}
      />
    </span>
  );
}
