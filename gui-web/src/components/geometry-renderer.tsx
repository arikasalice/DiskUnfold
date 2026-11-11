import { useId, useRef } from "react";
import type { KeyboardEvent, ReactNode } from "react";
import { Action } from "./controls";
import { Notice } from "./presentation";
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
): DisplayPoint {
  return {
    x: Math.round(((point.x - view.left) * width) / (view.right - view.left)),
    y: Math.round(((point.y - view.top) * height) / (view.bottom - view.top)),
  };
}
function primitive(
  geometry: DisplayGeometry,
  view: Viewport,
  width: number,
  height: number,
) {
  const points = geometry.points.map((point) =>
    project(point, view, width, height),
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
          x={start.x}
          y={start.y}
          width={end.x - start.x}
          height={end.y - start.y}
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
    event.nativeEvent.isComposing ||
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

function rectangleLabel(
  shape: Shape,
  view: Viewport,
  width: number,
  height: number,
) {
  if (shape.geometry.primitive !== "rect") return null;
  const start = project(shape.geometry.points[0]!, view, width, height);
  const end = project(shape.geometry.points[1]!, view, width, height);
  const left = Math.max(start.x, 0);
  const top = Math.max(start.y, 0);
  const available = Math.min(end.x, width) - left;
  if (available < 70 || Math.min(end.y, height) - top < 36) return null;
  const limit = Math.max(4, Math.floor((available - 24) / 14));
  const displayLabel = shape.displayLabel ?? shape.accessibleLabel;
  const label = Array.from(displayLabel);
  const visible =
    label.length > limit
      ? label.slice(0, limit - 1).join("") + "…"
      : displayLabel;
  return (
    <text className="du-geometry-label" x={left + 12} y={top + 24}>
      {visible}
    </text>
  );
}

/** Draws all supported common primitives or presents the supplied same-model table. */
export function GeometryRenderer(props: GeometryRendererProps) {
  const clip = useId();
  const controls = useRef(new Map<string, HTMLButtonElement>());
  const { shapes, relations, viewport, fallback, modelRef, resultRevision } =
    props;
  const fallbackMatches =
    fallback.modelRef === modelRef &&
    fallback.resultRevision === resultRevision;
  const valid =
    viewportValid(viewport, props.width, props.height) &&
    modelSupported(shapes, relations);
  if (!fallbackMatches)
    return <Notice tone="error">{props.missingFallbackNotice}</Notice>;
  if (!valid)
    return (
      <section aria-label={props.label}>
        <Notice>{props.fallbackNotice}</Notice>
        {fallback.content}
      </section>
    );
  const width = Math.round(props.width);
  const height = Math.round(props.height);
  const visible = shapes.flatMap((shape) => {
    if (!intersects(shape.geometry, viewport)) return [];
    const drawing = primitive(shape.geometry, viewport, width, height);
    return drawing === null ? [] : [{ shape, drawing }];
  });
  return (
    <section aria-label={props.label}>
      <svg
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
        <g clipPath={`url(#${clip})`}>
          {relations
            .filter(
              (relation) =>
                relation.geometry != null &&
                intersects(relation.geometry, viewport),
            )
            .map((relation, index) => (
              <g className="du-geometry-relation" key={`relation-${index}`}>
                {primitive(relation.geometry!, viewport, width, height)}
              </g>
            ))}
          {visible.map(({ shape, drawing }) => (
            <g
              className="du-geometry-shape"
              key={shape.shapeRef}
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
              {rectangleLabel(shape, viewport, width, height)}
            </g>
          ))}
        </g>
      </svg>
      <div role="group" aria-label={props.label} onKeyDown={moveFocus}>
        {visible.map(({ shape }) => (
          <Action
            key={shape.shapeRef}
            ref={(element) => {
              if (element) controls.current.set(shape.shapeRef, element);
              else controls.current.delete(shape.shapeRef);
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
