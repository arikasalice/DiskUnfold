import { useId, useLayoutEffect, useRef } from "react";
import type { KeyboardEvent, ReactNode } from "react";
import { Action } from "../controls";
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
  /** A lower-bound value: its right and bottom edges are dashed, since it may be larger. */
  lowerBound?: boolean;
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
  onFocusTargetMissing?: () => void;
  onFocusAvailability?: (available: boolean) => void;
  focusFirstRequest?: number;
  fallback: Fallback;
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
function moveFocus(event: KeyboardEvent<HTMLDivElement>) {
  if (
    isImeProcessingKey(event.nativeEvent) ||
    !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)
  )
    return;
  const buttons = Array.from(event.currentTarget.querySelectorAll("button"));
  const index = buttons.indexOf(event.target as HTMLButtonElement);
  if (index < 0 || buttons.length === 0) return;
  const offset =
    event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : -1;
  const next = (index + offset + buttons.length) % buttons.length;
  event.preventDefault();
  buttons[next]?.focus();
}

/**
 * A text cut to `limit` characters with the ellipsis in the middle, so the end
 * of a name (its extension) stays: "notes.bin" in 6 reads "no…bin" (F06 design
 * README section 7). A grapheme cluster is never split.
 */
function shorten(text: string, limit: number) {
  const characters = graphemes(text);
  if (characters.length <= limit) return text;
  const kept = Math.max(1, limit - 1);
  const tail = Math.ceil(kept / 2);
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

function rectangleLabel(
  shape: Shape,
  view: Viewport,
  width: number,
  height: number,
  grid: DeviceGrid,
) {
  if (shape.geometry.primitive !== "rect") return null;
  const start = project(shape.geometry.points[0]!, view, width, height, grid);
  const end = project(shape.geometry.points[1]!, view, width, height, grid);
  const left = Math.max(start.x, 0);
  const top = Math.max(start.y, 0);
  const available = Math.min(end.x, width) - left;
  // The header band of a folder: 20 px high, 12 px text 4 px from the left, centred.
  if (shape.container)
    return available < 70 || start.y < 0 ? null : (
      <text className="du-geometry-header" x={left + 4} y={top + 10}>
        {fitLabel(shape, Math.max(4, Math.floor((available - 8) / 12)))}
      </text>
    );
  if (available < 70 || Math.min(end.y, height) - top < 36) return null;
  const limit = Math.max(4, Math.floor((available - 24) / 14));
  const visible = fitLabel(shape, limit);
  return (
    <text className="du-geometry-label" x={left + 12} y={top + 24}>
      {visible}
    </text>
  );
}

/** Draws all supported common primitives or presents the supplied same-model table. */
export function GeometryRenderer(props: GeometryRendererProps) {
  const clip = useId();
  const svg = useRef<SVGSVGElement>(null);
  // Where the chart falls on device pixels, so its 1 px lines stay sharp (#156).
  const grid = useDeviceGrid(svg, Math.round(props.width));
  const controls = useRef(new Map<string, HTMLButtonElement>());
  const restoreFocus = useRef(false);
  const lastFocusFirstRequest = useRef(props.focusFirstRequest);
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
      props.focusFirstRequest !== lastFocusFirstRequest.current &&
      firstTarget
    )
      controls.current
        .get(firstTarget.shapeRef)
        ?.focus({ preventScroll: true });
    lastFocusFirstRequest.current = props.focusFirstRequest;
  }, [props.focusFirstRequest]);
  useLayoutEffect(() => {
    if (
      restoreFocus.current &&
      (document.activeElement === document.body ||
        !document.activeElement?.isConnected)
    ) {
      const matches = props.focusedRowRef
        ? props.shapes.filter((shape) => shape.rowRef === props.focusedRowRef)
        : [];
      const target =
        matches.length === 1
          ? controls.current.get(matches[0]!.shapeRef)
          : undefined;
      if (target) target.focus({ preventScroll: true });
      else props.onFocusTargetMissing?.();
    }
    restoreFocus.current = false;
    return () => {
      restoreFocus.current ||= Array.from(controls.current.values()).some(
        (button) => button === document.activeElement,
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
  return (
    <section aria-label={props.label}>
      <svg
        ref={svg}
        className="du-geometry"
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
        </defs>
        <g
          clipPath={`url(#${clip})`}
          transform={
            grid.dx || grid.dy ? `translate(${grid.dx} ${grid.dy})` : undefined
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
          {visible.map(({ shape, drawing }) => (
            <g
              className="du-geometry-shape"
              key={shape.shapeRef}
              data-container={shape.container ? "true" : undefined}
              onClick={() => {
                controls.current
                  .get(shape.shapeRef)
                  ?.focus({ preventScroll: true });
                props.onSelect(shape.rowRef, shape.shapeRef);
              }}
              data-selected={
                shape.rowRef === props.selectedRowRef ? "true" : undefined
              }
            >
              {drawing}
              {shape.rowRef === props.selectedRowRef &&
                selectionBand(shape, viewport, width, height, grid)}
              {lowerBoundEdge(shape, viewport, width, height, grid)}
              {rectangleLabel(shape, viewport, width, height, grid)}
            </g>
          ))}
        </g>
      </svg>
      <div
        className="du-geometry-buttons"
        role="group"
        aria-label={props.label}
        onKeyDown={moveFocus}
      >
        {visible.map(({ shape }) => (
          <Action
            key={`${modelRef}:${resultRevision}:${shape.shapeRef}`}
            onFocus={({ currentTarget }) => {
              props.onShapeFocus?.(
                shape.rowRef,
                shape.shapeRef,
                currentTarget instanceof HTMLElement
                  ? currentTarget
                  : undefined,
              );
              // The row scrolls sideways; the focused button must be in view.
              if (currentTarget instanceof HTMLElement)
                currentTarget.scrollIntoView?.({
                  block: "nearest",
                  inline: "nearest",
                });
            }}
            onBlur={() => {
              restoreFocus.current = false;
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
            aria-label={`${shape.accessibleLabel}${shape.rowRef === props.selectedRowRef ? `, ${props.selectedText}` : ""}`}
            onPress={() => props.onSelect(shape.rowRef, shape.shapeRef)}
          >
            {shape.displayLabel ?? shape.accessibleLabel}
          </Action>
        ))}
      </div>
      {fallback.content}
    </section>
  );
}
