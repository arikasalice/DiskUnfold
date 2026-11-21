import { useId, useLayoutEffect, useRef, useState } from "react";
import { ChartPatternDefs, patternId } from "./chart-patterns";
import type { ChartPattern } from "./chart-patterns";
import type { KeyboardEvent, ReactNode } from "react";
import { isImeProcessingKey } from "../ime-composition";
import { Notice } from "../presentation";
import { graphemes } from "../graphemes";
import { NO_GRID, snap, useDeviceGrid, whole } from "./device-grid";
import type { DeviceGrid } from "./device-grid";
import "./geometry-renderer.css";

/** Presentation coordinates; the caller adapts the generated model at its boundary. */
export type DisplayPoint = { readonly x: number; readonly y: number };
/** Read-only projection used only to draw a supplied primitive. */
export type DisplayGeometry = {
  readonly primitive: string;
  readonly points: readonly DisplayPoint[];
};
const LIMIT = 1_000_000;

type Shape = {
  shapeRef: string;
  rowRef: string;
  geometry: DisplayGeometry;
  accessibleLabel: string;
  displayLabel?: string;
  /** The drawn name and value apart, so a label that does not fit drops the value whole. */
  displayName?: string;
  displayValue?: string;
  /** An expanded folder: drawn as a frame with a header band over the shapes it holds. */
  container?: boolean;
  /** A folder, expanded or not; a bar chart marks its name with the folder icon (#318). */
  folder?: boolean;
  /** A group of items, such as the bar chart's "Other n" (F06 README 47). */
  group?: boolean;
  /** A border of dashes: a group narrowed to one type (README 47.4, 4.6). */
  dashed?: boolean;
  /** Said on hover and to assistive technology besides the name, such as a bar drawn shorter. */
  note?: string;
  /** A lower-bound value: its right and bottom edges are dashed, since it may be larger. */
  lowerBound?: boolean;
  /**
   * The palette place ("1"-"8"), "neutral", "neutral-2" or "folder" of its
   * legend class (F06 README 4.1); absent: the chart's own fill.
   */
  tone?: string;
  /** The pattern of its class, drawn when patterns are on or always. */
  pattern?: ChartPattern | null;
  patternAlways?: boolean;
  /** Another legend class is chosen: drawn faded, still selectable (README 4.7). */
  dimmed?: boolean;
  /** The key of its legend class, for choosing a class in the legend. */
  legendClass?: string;
};
type Relation = {
  fromShapeRef: string;
  toShapeRef: string;
  geometry?: DisplayGeometry | null;
};
type Viewport = { left: number; top: number; right: number; bottom: number };
type Fallback = {
  modelRef: string;
  resultRevision: string;
  content: ReactNode;
};

/** Presentation-only input; row and model references remain owned by the caller. */
export type GeometryRendererProps = {
  modelRef: string;
  resultRevision: string;
  label: string;
  selectedText: string;
  fallbackNotice: string;
  missingFallbackNotice: string;
  width: number;
  height: number;
  viewport: Viewport;
  shapes: readonly Shape[];
  relations: readonly Relation[];
  selectedRowRef: string | null;
  onSelect: (rowRef: string, shapeRef: string) => void;
  /** Reports the focused shape and its actual control when the renderer supplies one. */
  onShapeFocus?: (
    rowRef: string,
    shapeRef: string,
    control?: HTMLElement,
  ) => void;
  focusedRowRef?: string | null;
  /** Enter on a shape; `container` is true for an expanded folder (README 8, 34). */
  onActivate?: (rowRef: string, shapeRef: string, container: boolean) => void;
  /** Esc while a shape is selected. */
  onClearSelection?: () => void;
  /** The focused shape left the chart and the focus moved to a shape nearby (README 34). */
  onFocusMoved?: () => void;
  /** The focused shape left the chart and no shape is left to take the focus. */
  onFocusTargetMissing?: () => void;
  onFocusAvailability?: (available: boolean) => void;
  fallback: Fallback;
  /** Text drawn over the shapes in chart pixels, such as the names and values of a bar chart. */
  decorations?: ReactNode;
  /** Said of a bar drawn shorter than the axis (F06 README 47): "Drawn shorter than scale". */
  drawnShorterNote?: string;
  /** The short mark after such a bar's value (README 47, 3번 고침): "not to scale". */
  notToScaleMark?: string;
  /**
   * "ranked" fills shapes with the first chart colour and leaves their inner labels
   * out; the caller draws names and values beside them (bar chart, histogram).
   */
  variant?: "ranked";
  /** Draw the patterns of palette places 1-8 too ("Use patterns", README 4.1). */
  patterns?: boolean | undefined;
  /**
   * Every interval of a histogram in model order, shape or not; the histogram
   * keeps the place and name of an empty one (F06 README 43).
   */
  ranges?: readonly HistogramRange[] | undefined;
  /**
   * Places named for assistive technology that hold no shape, such as an empty
   * histogram interval: listed with the shapes but neither focused nor chosen.
   */
  inertOptions?: readonly InertOption[];
};

/** One histogram interval as the table names it. */
export type HistogramRange = {
  /** The row reference its shape carries when it has one. */
  rowRef: string;
  name: string;
  value: string;
  accessibleLabel: string;
};

/** A named place in chart pixels without a shape. */
export type InertOption = {
  key: string;
  label: string;
  box: { left: number; top: number; right: number; bottom: number };
};

function isPoint(value: DisplayPoint): boolean {
  return (
    value !== null &&
    typeof value === "object" &&
    Number.isInteger(value.x) &&
    Number.isInteger(value.y) &&
    value.x >= 0 &&
    value.x <= LIMIT &&
    value.y >= 0 &&
    value.y <= LIMIT
  );
}
function supported(geometry: DisplayGeometry): boolean {
  if (
    !geometry ||
    !Array.isArray(geometry.points) ||
    !geometry.points.every(isPoint)
  )
    return false;
  const points = geometry.points;
  switch (geometry.primitive) {
    case "point":
      return points.length === 1;
    case "rect":
      return (
        points.length === 2 &&
        points[0]!.x < points[1]!.x &&
        points[0]!.y < points[1]!.y
      );
    case "polyline":
      return points.length >= 2;
    case "polygon":
      return (
        points.length >= 3 &&
        (points[0]!.x !== points[points.length - 1]!.x ||
          points[0]!.y !== points[points.length - 1]!.y)
      );
    default:
      return false;
  }
}
function viewportValid(view: Viewport, width: number, height: number): boolean {
  return (
    view != null &&
    Number.isSafeInteger(Math.round(width)) &&
    Number.isSafeInteger(Math.round(height)) &&
    Math.round(width) > 0 &&
    Math.round(height) > 0 &&
    width <= Number.MAX_SAFE_INTEGER / LIMIT &&
    height <= Number.MAX_SAFE_INTEGER / LIMIT &&
    isPoint({ x: view.left, y: view.top }) &&
    isPoint({ x: view.right, y: view.bottom }) &&
    view.left < view.right &&
    view.top < view.bottom
  );
}
function modelSupported(
  shapes: readonly Shape[],
  relations: readonly Relation[],
): boolean {
  const refs = new Set<string>();
  for (const shape of shapes) {
    if (
      !shape ||
      typeof shape.shapeRef !== "string" ||
      typeof shape.rowRef !== "string" ||
      typeof shape.accessibleLabel !== "string" ||
      !shape.shapeRef ||
      !shape.rowRef ||
      !shape.accessibleLabel ||
      refs.has(shape.shapeRef) ||
      !supported(shape.geometry)
    )
      return false;
    refs.add(shape.shapeRef);
  }
  return relations.every(
    (relation) =>
      relation != null &&
      refs.has(relation.fromShapeRef) &&
      refs.has(relation.toShapeRef) &&
      (relation.geometry == null || supported(relation.geometry)),
  );
}
function intersects(geometry: DisplayGeometry, view: Viewport): boolean {
  let minX = LIMIT,
    minY = LIMIT,
    maxX = 0,
    maxY = 0;
  for (const point of geometry.points) {
    minX = Math.min(minX, point.x);
    maxX = Math.max(maxX, point.x);
    minY = Math.min(minY, point.y);
    maxY = Math.max(maxY, point.y);
  }
  return (
    maxX >= view.left &&
    minX <= view.right &&
    maxY >= view.top &&
    minY <= view.bottom
  );
}
function project(
  point: DisplayPoint,
  view: Viewport,
  width: number,
  height: number,
  grid: DeviceGrid = NO_GRID,
): DisplayPoint {
  return {
    x: snap(((point.x - view.left) * width) / (view.right - view.left), grid),
    y: snap(((point.y - view.top) * height) / (view.bottom - view.top), grid),
  };
}

/** How far inside its rectangle a shape is drawn: from the left and top, and from the right and bottom. */
type Inset = { start: number; end: number };
const NO_INSET: Inset = { start: 0, end: 0 };

/**
 * The gap between shapes, as whole device pixels. A leaf gives half of a 1 px
 * gap to each side, so two neighbours are one gap apart; an expanded folder is
 * drawn 1 px inside all round, so its inset shows its own fill (README 27, 4.7).
 */
function insetOf(shape: Shape, grid: DeviceGrid): Inset {
  if (shape.container) {
    const all = whole(1, grid);
    return { start: all, end: all };
  }
  const gap = Math.max(1, Math.round(grid.scale));
  const start = Math.floor(gap / 2) / grid.scale;
  return { start, end: gap / grid.scale - start };
}

/** The drawn edges of a rectangle shape after its inset, on device pixels. */
function drawnEdges(
  shape: Shape,
  view: Viewport,
  width: number,
  height: number,
  grid: DeviceGrid,
) {
  if (shape.geometry.primitive !== "rect") return null;
  const inset = insetOf(shape, grid);
  const start = project(shape.geometry.points[0]!, view, width, height, grid);
  const end = project(shape.geometry.points[1]!, view, width, height, grid);
  return {
    left: start.x + inset.start,
    top: start.y + inset.start,
    right: end.x - inset.end,
    bottom: end.y - inset.end,
  };
}
function primitive(
  geometry: DisplayGeometry,
  view: Viewport,
  width: number,
  height: number,
  grid: DeviceGrid = NO_GRID,
  inset: Inset = NO_INSET,
) {
  const points = geometry.points.map((point) =>
    project(point, view, width, height, grid),
  );
  const text = points.map((point) => `${point.x},${point.y}`).join(" ");
  switch (geometry.primitive) {
    case "point":
      return <circle cx={points[0]!.x} cy={points[0]!.y} r={2} />;
    case "rect": {
      const [start, end] = points;
      if (
        !start ||
        !end ||
        Math.min(end.x, width) <= Math.max(start.x, 0) ||
        Math.min(end.y, height) <= Math.max(start.y, 0)
      )
        return null;
      return (
        <rect
          x={start.x + inset.start}
          y={start.y + inset.start}
          width={Math.max(0, end.x - start.x - inset.start - inset.end)}
          height={Math.max(0, end.y - start.y - inset.start - inset.end)}
        />
      );
    }
    case "polyline":
      return <polyline points={text} fill="none" stroke="currentColor" />;
    case "polygon":
      return <polygon points={text} />;
    default:
      return null;
  }
}
type Box = { left: number; top: number; right: number; bottom: number };

/** Where a shape takes pointer and keyboard focus over the drawing, in chart pixels. */
function boxOf(
  shape: Shape,
  view: Viewport,
  width: number,
  height: number,
  grid: DeviceGrid,
): Box | null {
  const edges = drawnEdges(shape, view, width, height, grid);
  const points = shape.geometry.points.map((point) =>
    project(point, view, width, height, grid),
  );
  const box = edges ?? {
    left: Math.min(...points.map((point) => point.x)) - 4,
    top: Math.min(...points.map((point) => point.y)) - 4,
    right: Math.max(...points.map((point) => point.x)) + 4,
    bottom: Math.max(...points.map((point) => point.y)) + 4,
  };
  const clipped = {
    left: Math.max(0, box.left),
    top: Math.max(0, box.top),
    right: Math.min(width, box.right),
    bottom: Math.min(height, box.bottom),
  };
  return clipped.right > clipped.left && clipped.bottom > clipped.top
    ? clipped
    : null;
}

/**
 * The shape nearest in the direction of an arrow key on screen, or none at the
 * edge: the focus stops there (README 10, 22). Nearness is the gap between the
 * facing edges; a shape level with this one goes first, one off to the side
 * pays twice its offset. An expanded folder stands where its header band is,
 * so a header and the first row inside it are neighbours (README 35). Among
 * shapes as near (within the 3 px that insets and gaps make) it takes the shape
 * focused just before, then the one sharing the longest edge, then the upper,
 * then the left one (#186).
 */
function nearestInDirection(
  boxes: readonly Box[],
  from: number,
  key: string,
  previous?: number,
) {
  const centre = (box: Box) => ({
    x: (box.left + box.right) / 2,
    y: (box.top + box.bottom) / 2,
  });
  const origin = boxes[from]!;
  const start = centre(origin);
  const horizontal = key === "ArrowLeft" || key === "ArrowRight";
  const candidates = boxes.flatMap((box, index) => {
    if (index === from) return [];
    const point = centre(box);
    const dx = point.x - start.x;
    const dy = point.y - start.y;
    const along =
      key === "ArrowRight"
        ? dx
        : key === "ArrowLeft"
          ? -dx
          : key === "ArrowDown"
            ? dy
            : -dy;
    if (along <= 0.5) return [];
    const gap =
      key === "ArrowRight"
        ? box.left - origin.right
        : key === "ArrowLeft"
          ? origin.left - box.right
          : key === "ArrowDown"
            ? box.top - origin.bottom
            : origin.top - box.bottom;
    // How long the two share an edge across the direction of travel.
    const shared = horizontal
      ? Math.min(box.bottom, origin.bottom) - Math.max(box.top, origin.top)
      : Math.min(box.right, origin.right) - Math.max(box.left, origin.left);
    const distance =
      Math.max(0, gap) + (shared > 0 ? 0 : 2 * Math.abs(horizontal ? dy : dx));
    return [{ index, box, distance, shared }];
  });
  const nearest = Math.min(...candidates.map(({ distance }) => distance));
  const ties = candidates.filter(({ distance }) => distance <= nearest + 3);
  if (ties.some(({ index }) => index === previous)) return previous;
  ties.sort(
    (a, b) =>
      b.shared - a.shared || a.box.top - b.box.top || a.box.left - b.box.left,
  );
  return ties[0]?.index;
}

/** Where a shape stands for arrow keys: an expanded folder by its header band. */
function placeOf(shape: Shape, box: Box): Box {
  return shape.container
    ? { ...box, bottom: Math.min(box.bottom, box.top + HEADER_PLACE) }
    : box;
}
/** The 20 px header band less the inset of the shapes under it. */
const HEADER_PLACE = 18;

/**
 * A text cut to `limit` characters with the ellipsis in the middle, so the end
 * of a name (its extension) stays: "notes.bin" in 6 reads "no…bin" (F06 design
 * README section 7). A grapheme cluster is never split.
 */
/** A shortened name that keeps fewer leading characters than this is not drawn. */
const KEPT_HEAD = 3;

/**
 * The text shortened in the middle to `limit` characters, or "" when it would
 * keep fewer than three of its first characters: "R…ME" cannot be recognised,
 * so no label is better (F06 README 43, 44).
 */
function shorten(text: string, limit: number) {
  const characters = graphemes(text);
  if (characters.length <= limit) return text;
  const kept = Math.max(1, limit - 1);
  const tail = Math.ceil(kept / 2);
  if (kept - tail < KEPT_HEAD) return "";
  return (
    characters.slice(0, kept - tail).join("") +
    "…" +
    characters.slice(characters.length - tail).join("")
  );
}

/**
 * The label that fits `limit` characters. A shape that gives its name and
 * value apart shows both, or, when they do not fit, the name alone and shortened
 * if need be; the value is never cut in the middle.
 */
function fitLabel(shape: Shape, limit: number) {
  if (shape.displayName === undefined)
    return shorten(shape.displayLabel ?? shape.accessibleLabel, limit);
  const whole =
    shape.displayValue === undefined
      ? shape.displayName
      : `${shape.displayName} · ${shape.displayValue}`;
  return Array.from(whole).length <= limit
    ? whole
    : shorten(shape.displayName, limit);
}

/**
 * The dashed right and bottom edge of a lower-bound rectangle (F06 design README
 * 4.6): a 2 px band of whole device pixels just inside the drawn edge, which in an
 * expanded folder is its inset between its own edge and its children.
 */
function lowerBoundEdge(
  shape: Shape,
  view: Viewport,
  width: number,
  height: number,
  grid: DeviceGrid,
) {
  const edges = shape.lowerBound
    ? drawnEdges(shape, view, width, height, grid)
    : null;
  const band = whole(2, grid);
  if (!edges || edges.right - band <= edges.left) return null;
  if (edges.bottom - band <= edges.top) return null;
  const x = edges.right - band / 2;
  const y = edges.bottom - band / 2;
  return (
    <path
      className="du-geometry-lower-bound"
      strokeWidth={band}
      d={`M${x},${edges.top} V${y} H${edges.left}`}
    />
  );
}

/** The selection outline: a 3 px band of whole device pixels inside the drawn edge (4.7). */
function selectionBand(
  shape: Shape,
  view: Viewport,
  width: number,
  height: number,
  grid: DeviceGrid,
) {
  const edges = drawnEdges(shape, view, width, height, grid);
  const band = whole(3, grid);
  if (!edges || edges.right - edges.left <= band) return null;
  if (edges.bottom - edges.top <= band) return null;
  return (
    <rect
      className="du-geometry-selection"
      strokeWidth={band}
      x={edges.left + band / 2}
      y={edges.top + band / 2}
      width={edges.right - edges.left - band}
      height={edges.bottom - edges.top - band}
    />
  );
}

/** The selection mark: a check in the top right corner inside the band (4.7). */
function selectionCheck(
  shape: Shape,
  view: Viewport,
  width: number,
  height: number,
  grid: DeviceGrid,
) {
  const edges = drawnEdges(shape, view, width, height, grid);
  const size = whole(14, grid);
  const band = whole(3, grid);
  if (!edges || edges.right - edges.left < 2 * size + band) return null;
  if (edges.bottom - edges.top < 2 * size + band) return null;
  const x = edges.right - band - size;
  const y = edges.top + band;
  return (
    <g className="du-geometry-check">
      <rect x={x} y={y} width={size} height={size} />
      <path
        d={`M${x + size * 0.25},${y + size * 0.52} L${x + size * 0.43},${y + size * 0.7} L${x + size * 0.76},${y + size * 0.32}`}
      />
    </g>
  );
}

function rectangleLabel(
  shape: Shape,
  view: Viewport,
  width: number,
  height: number,
  grid: DeviceGrid,
  /** The shape draws a pattern: the label stands on a plate of its own fill. */
  plate = false,
) {
  if (shape.geometry.primitive !== "rect") return null;
  const start = project(shape.geometry.points[0]!, view, width, height, grid);
  const end = project(shape.geometry.points[1]!, view, width, height, grid);
  const left = Math.max(start.x, 0);
  const top = Math.max(start.y, 0);
  const available = Math.min(end.x, width) - left;
  // The header band of a folder: 20 px high, 12 px text 4 px from the left, centred.
  if (shape.container)
    return available < 70 || start.y < 0
      ? null
      : labelText(
          "du-geometry-header",
          left + 4,
          top + 10,
          fitLabel(shape, Math.max(4, Math.floor((available - 8) / 12))),
        );
  if (available < 70 || Math.min(end.y, height) - top < 36) return null;
  const limit = Math.max(4, Math.floor((available - 24) / 14));
  const label = fitLabel(shape, limit);
  const text = labelText("du-geometry-label", left + 12, top + 24, label);
  if (!plate || text === null) return text;
  return <PlatedLabel x={left + 12} y={top + 24} label={label} />;
}

/**
 * A label on a plate of its shape's fill, so no pattern runs through the name
 * (README 44): the 14 px line and 2 px above and below, the text and 4 px each
 * side. The plate takes the drawn width of the text, measured before paint; an
 * estimate stands where the text cannot be measured.
 */
function PlatedLabel({ x, y, label }: { x: number; y: number; label: string }) {
  const text = useRef<SVGTextElement>(null);
  const [width, setWidth] = useState(() => labelWidth(label));
  useLayoutEffect(() => {
    const measured = text.current?.getComputedTextLength?.();
    setWidth(measured && measured > 0 ? measured : labelWidth(label));
  }, [label]);
  return (
    <g>
      <rect
        className="du-geometry-label-plate"
        x={x - 4}
        y={y - 14 - 2}
        width={Math.ceil(width) + 8}
        height={14 + 4 + 4}
      />
      <text ref={text} className="du-geometry-label" x={x} y={y}>
        {label}
      </text>
    </g>
  );
}

const WIDE_GLYPH =
  /[\u1100-\u115f\u2e80-\ua4cf\uac00-\ud7a3\uf900-\ufaff\ufe30-\ufe4f\uff00-\uff60\uffe0-\uffe6]/u;

/** The estimated width of a 14 px label before it can be measured: 14 px for a wide glyph, 8 for others. */
function labelWidth(label: string) {
  return graphemes(label).reduce(
    (sum, part) => sum + (WIDE_GLYPH.test(part) ? 14 : 8),
    0,
  );
}

/** A drawn label, or nothing for a label that could not be kept. */
function labelText(className: string, x: number, y: number, label: string) {
  return label === "" ? null : (
    <text className={className} x={x} y={y}>
      {label}
    </text>
  );
}

/** Below this size in CSS px a pattern is not drawn (F06 README 4.1). */
const PATTERN_MIN = 24;

/**
 * The pattern of a shape's legend class over its fill, inside its gap: for every
 * class while patterns are on, and always for the neutral classes and the ninth
 * class onward. A folder frame and a small shape take none.
 */
function patternOverlay(
  shape: Shape,
  patterns: boolean | undefined,
  prefix: string,
  at: { viewport: Viewport; width: number; height: number; grid: DeviceGrid },
) {
  if (!shape.pattern || shape.container || !(patterns || shape.patternAlways))
    return null;
  const edges = drawnEdges(shape, at.viewport, at.width, at.height, at.grid);
  // The size rule counts the shape before its gap, so a 24 px bar takes its
  // pattern (#201).
  const inset = insetOf(shape, at.grid);
  const gap = inset.start + inset.end;
  if (
    !edges ||
    edges.right - edges.left + gap < PATTERN_MIN ||
    edges.bottom - edges.top + gap < PATTERN_MIN
  )
    return null;
  return (
    <rect
      className="du-geometry-pattern"
      x={edges.left}
      y={edges.top}
      width={edges.right - edges.left}
      height={edges.bottom - edges.top}
      fill={`url(#${patternId(prefix, shape.pattern)})`}
    />
  );
}

/** Draws all supported common primitives or presents the supplied same-model table. */
export function GeometryRenderer(props: GeometryRendererProps) {
  const clip = useId();
  const svg = useRef<SVGSVGElement>(null);
  // Where the chart falls on device pixels, so its 1 px lines stay sharp (#156).
  const grid = useDeviceGrid(svg, Math.round(props.width));
  const controls = useRef(new Map<string, HTMLElement>());
  const restoreFocus = useRef(false);
  // The model-order place of the focused shape, for "the same place" (README 22).
  const focusedIndex = useRef(0);
  // The shape focused before the current one, so an arrow back returns there (#186).
  const previousFocus = useRef<string | null>(null);
  const currentFocus = useRef<string | null>(null);
  const order = useRef<string[]>([]);
  // The shape that holds the chart's one Tab stop: the last focused one (README 10).
  const [tabStop, setTabStop] = useState<string | null>(null);
  const { shapes, relations, viewport, fallback, modelRef, resultRevision } =
    props;
  const fallbackMatches =
    fallback.modelRef === modelRef &&
    fallback.resultRevision === resultRevision;
  const valid =
    viewportValid(viewport, props.width, props.height) &&
    modelSupported(shapes, relations);
  const targetCounts = new Map<string, number>();
  for (const shape of shapes)
    targetCounts.set(shape.rowRef, (targetCounts.get(shape.rowRef) ?? 0) + 1);
  const firstTarget =
    valid && fallbackMatches
      ? shapes.find(
          (shape) =>
            targetCounts.get(shape.rowRef) === 1 &&
            intersects(shape.geometry, viewport) &&
            primitive(
              shape.geometry,
              viewport,
              Math.round(props.width),
              Math.round(props.height),
            ) !== null,
        )
      : undefined;
  useLayoutEffect(() => {
    props.onFocusAvailability?.(firstTarget !== undefined);
  }, [modelRef, resultRevision, firstTarget?.shapeRef]);
  useLayoutEffect(() => {
    if (
      restoreFocus.current &&
      (document.activeElement === document.body ||
        !document.activeElement?.isConnected)
    ) {
      // The same item, else the shape now at the same place, else the last
      // shape; with none left the caller takes the focus (README 22, 34).
      const matches = props.focusedRowRef
        ? props.shapes.filter((shape) => shape.rowRef === props.focusedRowRef)
        : [];
      const same =
        matches.length === 1
          ? controls.current.get(matches[0]!.shapeRef)
          : undefined;
      const refs = order.current;
      const nearby = refs[Math.min(focusedIndex.current, refs.length - 1)];
      const target =
        same ?? (nearby ? controls.current.get(nearby) : undefined);
      if (target) {
        target.focus({ preventScroll: true });
        if (!same) props.onFocusMoved?.();
      } else props.onFocusTargetMissing?.();
    }
    restoreFocus.current = false;
    return () => {
      restoreFocus.current ||= Array.from(controls.current.values()).some(
        (control) => control === document.activeElement,
      );
    };
  }, [props.modelRef, props.resultRevision]);
  if (!fallbackMatches)
    return <Notice tone="error">{props.missingFallbackNotice}</Notice>;
  if (!valid)
    return (
      <section aria-label={props.label}>
        <Notice>{props.fallbackNotice}</Notice>
        {fallback.content}
      </section>
    );
  if (shapes.length === 0)
    return <section aria-label={props.label}>{fallback.content}</section>;
  const width = Math.round(props.width);
  const height = Math.round(props.height);
  const visible = shapes.flatMap((shape) => {
    if (!intersects(shape.geometry, viewport)) return [];
    const drawing = primitive(
      shape.geometry,
      viewport,
      width,
      height,
      grid,
      insetOf(shape, grid),
    );
    return drawing === null ? [] : [{ shape, drawing }];
  });
  // Each drawn shape is one option of the chart's list, laid over its drawing.
  const targets = visible.flatMap(({ shape }) => {
    const box = boxOf(shape, viewport, width, height, grid);
    return box ? [{ shape, box }] : [];
  });
  order.current = targets.map(({ shape }) => shape.shapeRef);
  const stop = targets.some(({ shape }) => shape.shapeRef === tabStop)
    ? tabStop
    : (targets[0]?.shape.shapeRef ?? null);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (isImeProcessingKey(event.nativeEvent)) return;
    const index = targets.findIndex(
      ({ shape }) => controls.current.get(shape.shapeRef) === event.target,
    );
    const current = targets[index];
    if (!current) return;
    const { rowRef, shapeRef } = current.shape;
    let next: number | undefined;
    switch (event.key) {
      case "ArrowLeft":
      case "ArrowRight":
      case "ArrowUp":
      case "ArrowDown":
        next = nearestInDirection(
          targets.map(({ shape, box }) => placeOf(shape, box)),
          index,
          event.key,
          targets.findIndex(
            ({ shape }) => shape.shapeRef === previousFocus.current,
          ),
        );
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = targets.length - 1;
        break;
      case " ":
        event.preventDefault();
        props.onSelect(rowRef, shapeRef);
        return;
      case "Enter":
        event.preventDefault();
        props.onActivate?.(rowRef, shapeRef, current.shape.container === true);
        return;
      case "Escape":
        if (props.selectedRowRef !== null && props.onClearSelection) {
          event.preventDefault();
          props.onClearSelection();
        }
        return;
      default:
        return;
    }
    // At the edge the focus stays, and the page does not scroll instead.
    event.preventDefault();
    const target = next === undefined ? undefined : targets[next];
    if (target) controls.current.get(target.shape.shapeRef)?.focus();
  };
  return (
    <section aria-label={props.label}>
      <div className="du-geometry-frame">
        <svg
          ref={svg}
          className="du-geometry"
          data-variant={props.variant}
          aria-hidden="true"
          focusable="false"
          width={width}
          height={height}
          viewBox={`0 0 ${width} ${height}`}
          style={{ display: "block", maxWidth: "100%" }}
        >
          <defs>
            <clipPath id={clip}>
              <rect width={width} height={height} />
            </clipPath>
            <ChartPatternDefs prefix={clip} />
          </defs>
          <g
            clipPath={`url(#${clip})`}
            transform={
              grid.dx || grid.dy
                ? `translate(${grid.dx} ${grid.dy})`
                : undefined
            }
          >
            {relations
              .filter(
                (relation) =>
                  relation.geometry != null &&
                  intersects(relation.geometry, viewport),
              )
              .map((relation, index) => (
                <g className="du-geometry-relation" key={`relation-${index}`}>
                  {primitive(relation.geometry!, viewport, width, height, grid)}
                </g>
              ))}
            {visible.map(({ shape, drawing }) => {
              const selected = shape.rowRef === props.selectedRowRef;
              const overlay = patternOverlay(shape, props.patterns, clip, {
                viewport,
                width,
                height,
                grid,
              });
              return (
                <g
                  className="du-geometry-shape"
                  key={shape.shapeRef}
                  data-container={shape.container ? "true" : undefined}
                  data-selected={selected ? "true" : undefined}
                  data-tone={shape.tone}
                  data-dimmed={shape.dimmed ? "true" : undefined}
                  data-dashed={shape.dashed ? "true" : undefined}
                >
                  {drawing}
                  {overlay}
                  {selected &&
                    selectionBand(shape, viewport, width, height, grid)}
                  {lowerBoundEdge(shape, viewport, width, height, grid)}
                  {props.variant !== "ranked" &&
                    rectangleLabel(
                      shape,
                      viewport,
                      width,
                      height,
                      grid,
                      overlay !== null,
                    )}
                  {selected &&
                    selectionCheck(shape, viewport, width, height, grid)}
                </g>
              );
            })}
            {props.decorations}
          </g>
        </svg>
        <div
          className="du-geometry-targets"
          role="listbox"
          aria-label={props.label}
          onKeyDown={onKeyDown}
          style={
            grid.dx || grid.dy
              ? { transform: `translate(${grid.dx}px, ${grid.dy}px)` }
              : undefined
          }
        >
          {targets.map(({ shape, box }, index) => (
            <div
              key={`${modelRef}:${resultRevision}:${shape.shapeRef}`}
              className="du-geometry-target"
              role="option"
              data-shape-index={index}
              aria-selected={shape.rowRef === props.selectedRowRef}
              aria-label={shape.accessibleLabel}
              {...(shape.note
                ? { title: shape.note, "aria-description": shape.note }
                : {})}
              tabIndex={shape.shapeRef === stop ? 0 : -1}
              style={{
                left: box.left,
                top: box.top,
                width: box.right - box.left,
                height: box.bottom - box.top,
              }}
              ref={(element) => {
                if (element) controls.current.set(shape.shapeRef, element);
                else {
                  if (
                    controls.current.get(shape.shapeRef) ===
                    document.activeElement
                  )
                    restoreFocus.current = true;
                  controls.current.delete(shape.shapeRef);
                }
              }}
              onFocus={({ currentTarget }) => {
                focusedIndex.current = index;
                if (currentFocus.current !== shape.shapeRef) {
                  previousFocus.current = currentFocus.current;
                  currentFocus.current = shape.shapeRef;
                }
                setTabStop(shape.shapeRef);
                props.onShapeFocus?.(
                  shape.rowRef,
                  shape.shapeRef,
                  currentTarget,
                );
              }}
              onBlur={() => {
                restoreFocus.current = false;
              }}
              onClick={({ currentTarget }) => {
                currentTarget.focus({ preventScroll: true });
                props.onSelect(shape.rowRef, shape.shapeRef);
              }}
            />
          ))}
          {props.inertOptions?.map(({ key, label, box }) => (
            <div
              key={`inert:${key}`}
              className="du-geometry-target"
              role="option"
              aria-disabled="true"
              aria-selected={false}
              aria-label={label}
              style={{
                left: box.left,
                top: box.top,
                width: box.right - box.left,
                height: box.bottom - box.top,
              }}
            />
          ))}
        </div>
      </div>
      {fallback.content}
    </section>
  );
}
