import { GeometryRenderer } from "./geometry-renderer";
import type { GeometryRendererProps } from "./geometry-renderer";
import { shorten } from "./ranked-renderers";
import { SANKEY_COLUMNS } from "./sankey-links";

/** The engine's unit coordinate range (geometry v1). */
const LIMIT = 1_000_000;
/** Node width (F06 README 12): 12 CSS px. */
const NODE = 12;
/** Room right of the type column for "name · value · %", at most this or 30% of the width. */
const LABEL_ROOM = 240;
const LABEL_SHARE = 0.3;
/** Between a node and its label, and the label's line within the node's top. */
const LABEL_GAP = 6;
const LABEL_LINE = 16;
/** The band above the nodes for the column names (design, 2026-11-23). */
const HEADING = 20;

type Shape = GeometryRendererProps["shapes"][number];
type Relation = GeometryRendererProps["relations"][number];
type Geometry = Shape["geometry"];
type Span = { left: number; right: number };
type Column = (typeof SANKEY_COLUMNS)[number];
type Spans = { scope?: Span; folder: Span; type: Span };

const fullView = { left: 0, top: 0, right: LIMIT, bottom: LIMIT };

/**
 * A Sankey drawn by the rules of F06 README 12 and 12.1: folders and their
 * types in columns of 12 px nodes, the type nodes in their legend colours, the
 * folder nodes muted, and the links between them, under the names of the
 * columns. A single scope root carries nothing the folder column does not (its
 * path and total are in the title and status line), so it is left out and its
 * width goes to the links; several roots keep their own column. The engine lays
 * the nodes and links out across the whole width; this moves the columns apart
 * so the type column leaves room on its right for the labels, stretching the
 * links between them and keeping their heights.
 */
export function SankeyRenderer(props: GeometryRendererProps) {
  const width = Math.round(props.width);
  const height = Math.round(props.height);
  const view = props.viewport;
  const toPx = (x: number, y: number) => ({
    x: ((x - view.left) * width) / (view.right - view.left),
    y: ((y - view.top) * height) / (view.bottom - view.top),
  });
  const { shapes: shown, relations: links } = withoutLoneScope(props);
  const spans = columnSpans(shown, toPx);
  const room = Math.min(LABEL_ROOM, Math.floor(width * LABEL_SHARE));
  const mapX = spans ? stretch(spans, width - room) : null;
  if (!spans || !mapX || view.right <= view.left || view.bottom <= view.top)
    return <GeometryRenderer {...props} variant="sankey" />;
  const top = props.columnNames ? HEADING : 0;
  const place = (geometry: Geometry): Geometry => ({
    primitive: geometry.primitive,
    points: geometry.points.map((point) => {
      const at = toPx(point.x, point.y);
      return {
        x: unit(mapX(at.x), width),
        y: unit(top + (at.y * (height - top)) / height, height),
      };
    }),
  });
  const byRef = new Map(shown.map((shape) => [shape.shapeRef, shape]));
  const shapes = shown.map((shape) => {
    const placed: Shape = { ...shape, geometry: place(shape.geometry) };
    // A node is one bar of a column; the group border of "Other" or of a type
    // would read as a second edge on a 12 px node.
    delete placed.dashed;
    return placed;
  });
  const relations = links.map((relation) => linkLook(relation, byRef, place));
  const labelEnd = {
    scope: mapX(spans.folder.left) - LABEL_GAP,
    folder: mapX(spans.type.left) - LABEL_GAP,
    type: width,
  };
  const decorations = (
    <>
      {headings(spans, mapX, props.columnNames)}
      {labels(shapes, { width, height, labelEnd })}
    </>
  );
  return (
    <GeometryRenderer
      {...props}
      viewport={fullView}
      shapes={shapes}
      relations={relations}
      decorations={decorations}
      variant="sankey"
    />
  );
}

/** The shapes and links without the scope node when it is the only one. */
function withoutLoneScope(props: GeometryRendererProps) {
  const scopes = props.shapes.filter((shape) => shape.column === "scope");
  const lone = scopes.length === 1 ? scopes[0]!.shapeRef : null;
  if (lone === null) return props;
  return {
    shapes: props.shapes.filter((shape) => shape.shapeRef !== lone),
    relations: props.relations.filter(
      (relation) =>
        relation.fromShapeRef !== lone && relation.toShapeRef !== lone,
    ),
  };
}

/** The column names above the nodes, in the muted text colour. */
function headings(
  spans: Spans,
  mapX: (x: number) => number,
  names: GeometryRendererProps["columnNames"],
) {
  if (!names) return null;
  return SANKEY_COLUMNS.map((column) => {
    const span = spans[column];
    const name = names[column];
    return span && name ? (
      <text
        key={column}
        className="du-geometry-sankey-heading"
        x={mapX(span.left)}
        y={HEADING / 2}
      >
        {name}
      </text>
    ) : null;
  });
}

/**
 * The left and right edges of each column in chart pixels, or null without a
 * folder and a type column in order.
 */
function columnSpans(
  shapes: readonly Shape[],
  toPx: (x: number, y: number) => { x: number; y: number },
): Spans | null {
  const spans: Partial<Record<Column, Span>> = {};
  for (const shape of shapes) {
    const column = SANKEY_COLUMNS.find((name) => name === shape.column);
    const [start, end] = shape.geometry.points;
    if (!column || shape.geometry.primitive !== "rect" || !start || !end)
      continue;
    const left = toPx(start.x, start.y).x;
    const right = toPx(end.x, end.y).x;
    const span = spans[column];
    spans[column] = span
      ? { left: Math.min(span.left, left), right: Math.max(span.right, right) }
      : { left, right };
  }
  const { scope, folder, type } = spans;
  if (!folder || !type || folder.right > type.left) return null;
  if (scope && scope.right > folder.left) return null;
  return scope ? { scope, folder, type } : { folder, type };
}

/**
 * Maps the engine's x to the drawn x, piece by piece: each column becomes a
 * 12 px node, the first column at the left, the type column ending at
 * `typeRight` and a folder column between two others halfway; the links
 * between columns stretch to fit.
 */
function stretch(spans: Spans, typeRight: number) {
  const typeLeft = typeRight - NODE;
  const folderLeft = spans.scope
    ? Math.round((NODE + typeLeft) / 2 - NODE / 2)
    : 0;
  if (folderLeft + NODE >= typeLeft) return null;
  if (spans.scope && folderLeft <= NODE) return null;
  const stops: [number, number][] = [
    ...(spans.scope
      ? ([
          [spans.scope.left, 0],
          [spans.scope.right, NODE],
        ] as [number, number][])
      : []),
    [spans.folder.left, folderLeft],
    [spans.folder.right, folderLeft + NODE],
    [spans.type.left, typeLeft],
    [spans.type.right, typeRight],
  ];
  return (x: number) => {
    for (let index = 1; index < stops.length; index += 1) {
      const [fromX, toX] = stops[index - 1]!;
      const [nextX, nextTo] = stops[index]!;
      if (x <= nextX || index === stops.length - 1) {
        if (nextX === fromX) return x <= fromX ? toX : nextTo;
        return toX + ((x - fromX) * (nextTo - toX)) / (nextX - fromX);
      }
    }
    return x;
  };
}

function unit(value: number, span: number) {
  return Math.min(LIMIT, Math.max(0, Math.round((value / span) * LIMIT)));
}

/**
 * A link's band and colour (README 12.1): a band into a type node takes that
 * type's colour and pattern; a band into a folder takes the muted colour.
 */
function linkLook(
  relation: Relation,
  byRef: ReadonlyMap<string, Shape>,
  place: (geometry: Geometry) => Geometry,
): Relation {
  const target = byRef.get(relation.toShapeRef);
  const placed: Relation = {
    ...relation,
    ...(relation.geometry ? { geometry: place(relation.geometry) } : {}),
  };
  if (target?.column !== "type") return { ...placed, band: "scope" };
  return {
    ...placed,
    band: "type",
    ...(target.tone ? { tone: target.tone } : {}),
    ...(target.pattern ? { pattern: target.pattern } : {}),
    ...(target.patternAlways ? { patternAlways: true } : {}),
  };
}

/**
 * The labels right of the nodes, on the node's first line: the scope's name,
 * a folder's "name · value", a type's "name · value · share". A label that would
 * run into the one above it in its column, or a node lower than its line, is
 * left out; the node's info card and the legend still name it.
 */
function labels(
  shapes: readonly Shape[],
  at: {
    width: number;
    height: number;
    labelEnd: Record<Column, number>;
  },
) {
  const lastLine: Partial<Record<Column, number>> = {};
  const drawn = [...shapes]
    .map((shape) => ({ shape, box: pixels(shape.geometry, at) }))
    .filter(({ box }) => box !== null)
    .sort((a, b) => a.box!.top - b.box!.top);
  return drawn.map(({ shape, box }) => {
    const column = SANKEY_COLUMNS.find((name) => name === shape.column);
    if (!column || !box || box.bottom - box.top < LABEL_LINE / 2) return null;
    // A small node at the bottom keeps its label inside the chart.
    const y = Math.min(box.top + LABEL_LINE / 2, at.height - LABEL_LINE / 2);
    const previous = lastLine[column];
    if (previous !== undefined && y - previous < LABEL_LINE) return null;
    const words = labelWords(shape, column);
    const x = box.right + LABEL_GAP;
    const room = at.labelEnd[column] - x;
    const text = words === "" ? "" : shorten(words, room);
    if (text === "" || room <= 0) return null;
    lastLine[column] = y;
    return (
      <text
        key={shape.shapeRef}
        className="du-geometry-sankey-label"
        x={x}
        y={y}
      >
        {text}
      </text>
    );
  });
}

function labelWords(shape: Shape, column: Column) {
  const name = shape.displayName ?? shape.displayLabel ?? "";
  if (name === "" || column === "scope") return name;
  const parts = [name, shape.displayValue];
  if (column === "type") parts.push(shape.displayShare);
  return parts.filter((part) => part !== undefined && part !== "").join(" · ");
}

function pixels(
  geometry: Geometry,
  at: { width: number; height: number },
): { left: number; top: number; right: number; bottom: number } | null {
  const [start, end] = geometry.points;
  if (geometry.primitive !== "rect" || !start || !end) return null;
  return {
    left: (start.x / LIMIT) * at.width,
    top: (start.y / LIMIT) * at.height,
    right: (end.x / LIMIT) * at.width,
    bottom: (end.y / LIMIT) * at.height,
  };
}
