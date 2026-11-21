import type {
  ItemRef,
  VizModel,
  VizRenderResponse,
  VizRow,
  VizRows,
  VizRowsRequest,
  VizShape,
} from "../ipc";
import type { GeometryRendererProps } from "../components";
import type { ChartPattern } from "../components";
import type { ShapePlace } from "./model-text";

export type ModelResponse = Extract<VizRenderResponse, { kind: "model" }>;

/** Local query provenance: VizRows does not repeat its requested modelRef. */
export type ModelRowsPage = {
  request: VizRowsRequest;
  response: VizRows;
};

/** Selection is local view state, never an IPC approval or navigation request. */
export type VizSelection = {
  modelRef: VizModel["modelRef"];
  resultRevision: ModelResponse["resultRevision"];
} & (
  | { kind: "item"; itemRef: ItemRef; matchKey: VizShape["matchKey"] }
  | { kind: "group"; groupRef: string }
);

function targetKey(target: Pick<VizRow, "itemRef" | "groupRef">) {
  if ((target.itemRef === null) === (target.groupRef === null)) return null;
  return target.itemRef
    ? JSON.stringify([
        "item",
        target.itemRef.resultVersion,
        target.itemRef.itemNo,
      ])
    : JSON.stringify(["group", target.groupRef]);
}

/** The reference a renderer shape carries for the row or shape "target". */
export const rowTargetKey = targetKey;

export function selectionKey(selection: VizSelection, frame: ModelResponse) {
  if (selection.resultRevision !== frame.resultRevision) return null;
  if (selection.kind === "group" && selection.modelRef !== frame.model.modelRef)
    return null;
  return targetKey({
    itemRef: selection.kind === "item" ? selection.itemRef : null,
    groupRef: selection.kind === "group" ? selection.groupRef : null,
  });
}

export function matchingRowsPage(frame: ModelResponse, page: ModelRowsPage) {
  const { request, response } = page;
  return (
    request.modelRef === frame.model.modelRef &&
    request.resultRevision === frame.resultRevision &&
    response.resultRevision === frame.resultRevision &&
    response.observedAt === frame.observedAt &&
    response.completeness === frame.completeness &&
    response.rows.every(
      (row) =>
        targetKey(row) !== null &&
        (row.itemRef === null ||
          row.itemRef.resultVersion === frame.resultRevision),
    )
  );
}

/**
 * Keeps geometry, order and identity intact; it never computes layout or totals.
 * `folderRows` names the shapes inside expanded folders; a row of `page` wins
 * when both hold the same reference. Shapes come folder first, then what it
 * holds (depth first), the order of the table rows.
 */
export function adaptVizModel(
  frame: ModelResponse,
  page: ModelRowsPage | undefined,
  describe: (
    shape: VizShape,
    row: VizRow | undefined,
    place: ShapePlace,
  ) => {
    accessibleLabel: string;
    displayLabel: string;
    displayName?: string;
    displayValue?: string;
    container?: boolean;
    lowerBound?: boolean;
    tone?: string;
    pattern?: ChartPattern | null;
    patternAlways?: boolean;
    legendClass?: string;
    card?: { title: string; lines: readonly string[] };
  },
  folderRows: readonly VizRow[] = [],
) {
  const rowsMatch = page !== undefined && matchingRowsPage(frame, page);
  const rows = rowsMatch ? page.response.rows : [];
  const rowByTarget = new Map(
    [...folderRows, ...rows].map((row) => [targetKey(row), row]),
  );
  const targetsByShape = new Map<string, VizSelection>();
  // The item kind the engine supplied; absent when unknown (#183).
  const kindsByShape = new Map<string, NonNullable<VizShape["itemKind"]>>();
  let validTargets = true;
  const childCount = new Map<string, number>();
  const byRef = new Map<string, VizShape>();
  for (const shape of frame.model.shapes) {
    if (shape.shapeRef) byRef.set(shape.shapeRef, shape);
    if (shape.parentShapeRef)
      childCount.set(
        shape.parentShapeRef,
        (childCount.get(shape.parentShapeRef) ?? 0) + 1,
      );
  }
  const placeOf = (shape: VizShape): ShapePlace => {
    const parent = shape.parentShapeRef
      ? byRef.get(shape.parentShapeRef)
      : undefined;
    const parentKey = parent ? targetKey(parent) : null;
    const count = shape.shapeRef ? childCount.get(shape.shapeRef) : undefined;
    return {
      ...(count ? { childCount: count } : {}),
      ...(parent
        ? {
            inFolder: true,
            parentRow: parentKey ? rowByTarget.get(parentKey) : undefined,
          }
        : {}),
    };
  };
  const adapted: GeometryRendererProps["shapes"] = frame.model.shapes.map(
    (shape, index) => {
      const key = targetKey(shape);
      const shapeRef = visualizationShapeRef(shape.shapeRef, index);
      if (
        !key ||
        (shape.itemRef && shape.itemRef.resultVersion !== frame.resultRevision)
      )
        validTargets = false;
      if (key) {
        const base = {
          modelRef: frame.model.modelRef,
          resultRevision: frame.resultRevision,
        };
        if (shape.itemRef) {
          if (shape.itemKind) kindsByShape.set(shapeRef, shape.itemKind);
          targetsByShape.set(shapeRef, {
            ...base,
            kind: "item",
            itemRef: shape.itemRef,
            matchKey: shape.matchKey,
          });
        } else if (shape.groupRef !== null) {
          targetsByShape.set(shapeRef, {
            ...base,
            kind: "group",
            groupRef: shape.groupRef,
          });
        }
      }
      return {
        shapeRef,
        rowRef: key ?? "",
        geometry: shape.geometry,
        ...describe(
          shape,
          key ? rowByTarget.get(key) : undefined,
          placeOf(shape),
        ),
      };
    },
  );
  const shapes = depthFirst(frame.model.shapes).map((index) => adapted[index]!);
  const relations: GeometryRendererProps["relations"] =
    frame.model.relationships.map((relation) => ({
      fromShapeRef: visualizationShapeRef(relation.fromShapeRef, 0),
      toShapeRef: visualizationShapeRef(relation.toShapeRef, 0),
      ...(relation.geometry === undefined
        ? {}
        : { geometry: relation.geometry }),
    }));
  return {
    shapes,
    relations,
    targetsByShape,
    kindsByShape,
    rows,
    rowsMatch,
    validTargets,
  };
}

/** Indexes of `shapes` with each folder followed by the shapes it holds. */
function depthFirst(shapes: readonly VizShape[]): number[] {
  const held = new Map<string, number[]>();
  const refs = new Set(shapes.flatMap((shape) => shape.shapeRef ?? []));
  const roots: number[] = [];
  shapes.forEach((shape, index) => {
    const parent = shape.parentShapeRef;
    if (parent && refs.has(parent))
      held.set(parent, [...(held.get(parent) ?? []), index]);
    else roots.push(index);
  });
  const order: number[] = [];
  const seen = new Set<number>();
  const visit = (index: number) => {
    if (seen.has(index)) return;
    seen.add(index);
    order.push(index);
    const ref = shapes[index]!.shapeRef;
    for (const child of (ref && held.get(ref)) || []) visit(child);
  };
  roots.forEach(visit);
  // A shape on a reference cycle has no root; it keeps its place at the end.
  shapes.forEach((_, index) => visit(index));
  return order;
}

export function visualizationShapeRef(
  shapeRef: string | undefined,
  index: number,
) {
  return shapeRef === undefined
    ? JSON.stringify(["flat", index])
    : JSON.stringify(["shape", shapeRef]);
}
