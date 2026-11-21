/**
 * The drawn width of a text in the chart's font, measured on a 2D canvas (F06
 * README 51, #367). Where no canvas measures text, as in the DOM test
 * environment, the width is null and the caller keeps its estimate.
 */
type Measurer = {
  readonly maker: unknown;
  readonly context: OffscreenCanvasRenderingContext2D | null;
  readonly family: string;
  readonly widths: Map<string, number>;
};

/** Widths kept before the cache starts again; a chart draws far fewer labels. */
const KEPT_WIDTHS = 4096;

let measurer: Measurer | null = null;

/** The page font the chart text inherits (theme tokens, :root). */
function chartFamily() {
  if (typeof document === "undefined") return "sans-serif";
  const root = document.body ?? document.documentElement;
  return getComputedStyle(root).fontFamily || "sans-serif";
}

function current(): Measurer | null {
  const maker = globalThis.OffscreenCanvas;
  if (typeof maker !== "function") return null;
  if (measurer?.maker !== maker) {
    let context: OffscreenCanvasRenderingContext2D | null = null;
    try {
      context = new maker(1, 1).getContext("2d");
    } catch {
      context = null;
    }
    measurer = { maker, context, family: chartFamily(), widths: new Map() };
  }
  return measurer.context === null ? null : measurer;
}

/** The width in CSS px of `text` drawn at `px` in the chart font, or null. */
export function measuredWidth(text: string, px: number): number | null {
  const found = current();
  if (found === null || found.context === null) return null;
  const key = `${px}\u0000${text}`;
  const known = found.widths.get(key);
  if (known !== undefined) return known;
  found.context.font = `${px}px ${found.family}`;
  const width = found.context.measureText(text).width;
  if (found.widths.size >= KEPT_WIDTHS) found.widths.clear();
  found.widths.set(key, width);
  return width;
}
