import { GeometryRenderer } from "./geometry-renderer";
import type { GeometryRendererProps } from "./geometry-renderer";
import { graphemes } from "../graphemes";

/** The engine's unit coordinate range (geometry v1). */
const LIMIT = 1_000_000;
/** Bar chart rows (F06 README 12): bars 24 CSS px high with 12 between. */
const BAR = 24;
const GAP = 12;
const PAD = 4;
/** The name column takes at most this share of the chart width. */
const NAME_SHARE = 0.4;
/** Room after the longest bar for its value. */
const VALUE_ROOM = 96;
/** Histogram: room above a column for its value and below it for its bin name. */
const VALUE_BAND = 20;
const NAME_BAND = 24;
/** An approximate character width of the 12 px chart text, for shortening names. */
const CHAR = 7;

type Shape = GeometryRendererProps["shapes"][number];

/** The share of the value axis a shape covers: its rectangle's length across the view. */
function share(shape: Shape, view: GeometryRendererProps["viewport"]) {
  const [start, end] = shape.geometry.points;
  if (shape.geometry.primitive !== "rect" || !start || !end) return null;
  const span = view.right - view.left;
  return span > 0 ? Math.min(1, Math.max(0, (end.x - start.x) / span)) : null;
}

/** Keeps the start and the end of a name that is too long, as names are shortened elsewhere. */
function shorten(text: string, room: number) {
  const parts = graphemes(text);
  const limit = Math.max(2, Math.floor(room / CHAR));
  if (parts.length <= limit) return text;
  const tail = Math.floor((limit - 1) / 2);
  return (
    parts.slice(0, limit - 1 - tail).join("") +
    "…" +
    parts.slice(parts.length - tail).join("")
  );
}

/** A rectangle in chart pixels as geometry over a full unit view of the chart. */
function rect(
  left: number,
  top: number,
  right: number,
  bottom: number,
  width: number,
  height: number,
) {
  const x = (value: number) =>
    Math.min(LIMIT, Math.max(0, Math.round((value / width) * LIMIT)));
  const y = (value: number) =>
    Math.min(LIMIT, Math.max(0, Math.round((value / height) * LIMIT)));
  return {
    primitive: "rect",
    points: [
      { x: x(left), y: y(top) },
      { x: x(right), y: y(bottom) },
    ],
  };
}

/** The drawn name and value of a shape; its inner label is not drawn. */
function words(shape: Shape) {
  return {
    name: shape.displayName ?? shape.displayLabel ?? "",
    value: shape.displayValue ?? "",
  };
}

function bare(shape: Shape, geometry: Shape["geometry"]): Shape {
  const rest: Shape = { ...shape, geometry };
  delete rest.displayLabel;
  delete rest.displayName;
  delete rest.displayValue;
  return rest;
}

const fullView = { left: 0, top: 0, right: LIMIT, bottom: LIMIT };

/**
 * A ranked bar chart drawn by the rules of F06 README 12: horizontal bars in
 * model order (largest first, "Other n" last), 24 CSS px high with 12 between,
 * the name on the left in a column of at most 40% of the width and the value
 * to the right of the bar's end. The value axis starts at zero; a bar's length
 * is the share of the axis the engine gave it. Focus, selection and the keys
 * stay those of the shared geometry renderer.
 */
export function BarRenderer(props: GeometryRendererProps) {
  const width = Math.round(props.width);
  const nameWidth = Math.floor(width * NAME_SHARE);
  const plotLeft = nameWidth + 8;
  const plotWidth = Math.max(0, width - plotLeft - VALUE_ROOM);
  const rows = props.shapes.flatMap((shape) => {
    const part = share(shape, props.viewport);
    return part === null ? [] : [{ shape, part }];
  });
  const height = Math.max(
    BAR + 2 * PAD,
    PAD * 2 + rows.length * (BAR + GAP) - GAP,
  );
  const shapes = rows.map(({ shape, part }, index) => {
    const top = PAD + index * (BAR + GAP);
    const length = Math.max(2, Math.round(part * plotWidth));
    return bare(
      shape,
      rect(plotLeft, top, plotLeft + length, top + BAR, width, height),
    );
  });
  const decorations = rows.map(({ shape, part }, index) => {
    const middle = PAD + index * (BAR + GAP) + BAR / 2;
    const length = Math.max(2, Math.round(part * plotWidth));
    const { name, value } = words(shape);
    return (
      <g key={shape.shapeRef}>
        <text className="du-geometry-bar-name" x={PAD} y={middle}>
          {shorten(name, nameWidth - PAD)}
        </text>
        <text
          className="du-geometry-bar-value"
          x={plotLeft + length + 6}
          y={middle}
        >
          {value}
        </text>
      </g>
    );
  });
  return (
    <GeometryRenderer
      {...props}
      height={height}
      viewport={fullView}
      shapes={shapes}
      decorations={decorations}
    />
  );
}

/**
 * A histogram drawn by the rules of F06 README 12: one column per bin in bin
 * order, its height the share of the value axis the engine gave it from a zero
 * baseline, the bin's name ("1–10 MiB") below it and its value above it.
 */
export function HistogramRenderer(props: GeometryRendererProps) {
  const width = Math.round(props.width);
  const height = Math.round(props.height);
  const bins = props.shapes.flatMap((shape) => {
    const part = share(shape, props.viewport);
    return part === null ? [] : [{ shape, part }];
  });
  const plotTop = VALUE_BAND;
  const plotBottom = Math.max(plotTop + 1, height - NAME_BAND);
  const plotHeight = plotBottom - plotTop;
  const column = bins.length > 0 ? (width - 2 * PAD) / bins.length : 0;
  const gap = Math.min(GAP, Math.floor(column * 0.2));
  const place = (index: number, part: number) => {
    const left = Math.round(PAD + index * column + gap / 2);
    const right = Math.max(
      left + 2,
      Math.round(PAD + (index + 1) * column - gap / 2),
    );
    const top = plotBottom - Math.max(2, Math.round(part * plotHeight));
    return { left, right, top };
  };
  const shapes = bins.map(({ shape, part }, index) => {
    const { left, right, top } = place(index, part);
    return bare(shape, rect(left, top, right, plotBottom, width, height));
  });
  const decorations = (
    <>
      <line
        className="du-geometry-baseline"
        x1={PAD}
        x2={width - PAD}
        y1={plotBottom}
        y2={plotBottom}
      />
      {bins.map(({ shape, part }, index) => {
        const { left, right, top } = place(index, part);
        const centre = (left + right) / 2;
        const { name, value } = words(shape);
        return (
          <g key={shape.shapeRef}>
            <text className="du-geometry-bin-value" x={centre} y={top - 6}>
              {shorten(value, right - left + gap)}
            </text>
            <text
              className="du-geometry-bin-name"
              x={centre}
              y={plotBottom + NAME_BAND / 2}
            >
              {shorten(name, right - left + gap)}
            </text>
          </g>
        );
      })}
    </>
  );
  return (
    <GeometryRenderer
      {...props}
      viewport={fullView}
      shapes={shapes}
      decorations={decorations}
    />
  );
}
