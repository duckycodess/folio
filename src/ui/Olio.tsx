const FILES = import.meta.glob<string>("../assets/olio/*.png", {
  eager: true,
  query: "?url",
  import: "default",
});

export const OLIO_POSES = [
  "default",
  "waving",
  "reading",
  "organizing",
  "holding-document",
  "thinking",
  "confused",
  "worried",
  "success",
  "peeking",
  "celebrating",
  "sleeping",
] as const;

export type OlioPose = (typeof OLIO_POSES)[number];

/** Display sizes from docs/design.md: inline, header/panel, empty state. */
export type OlioSize = 48 | 96 | 160;

// Each pose is exported at 2× of every display size.
const SOURCES: Record<OlioSize, number[]> = {
  48: [96],
  96: [96, 192],
  160: [192, 320],
};

function url(pose: OlioPose, px: number): string {
  const file = FILES[`../assets/olio/olio-${pose}-${px}.png`];
  if (!file) throw new Error(`Missing Olio artwork: ${pose} at ${px}px`);
  return file;
}

/** Image sources for one pose at one display size. */
export function olioSource(pose: OlioPose, size: OlioSize) {
  const [small, large = small] = SOURCES[size];
  return {
    src: url(pose, large),
    srcSet:
      small === large
        ? undefined
        : `${url(pose, small)} 1x, ${url(pose, large)} 2x`,
  };
}

/**
 * The mascot, as provided. Always decorative: the text beside it carries
 * the meaning, so it is hidden from assistive technology.
 */
export function Olio({ pose, size }: { pose: OlioPose; size: OlioSize }) {
  const { src, srcSet } = olioSource(pose, size);
  return (
    <span className="olio" aria-hidden="true">
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
