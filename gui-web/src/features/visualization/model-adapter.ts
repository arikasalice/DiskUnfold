import type {
  ItemRef,
  VizModel,
  VizRenderResponse,
  VizRow,
  VizRows,
  VizRowsRequest,
  VizShape,
} from "../../ipc";
import type { GeometryRendererProps } from "../../components";

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

/** Keeps geometry, order and identity intact; it never computes layout or totals. */
export function adaptVizModel(
  frame: ModelResponse,
  page: ModelRowsPage | undefined,
  describe: (
    shape: VizShape,
    row: VizRow | undefined,
  ) => { accessibleLabel: string; displayLabel: string },
) {
  const rowsMatch = page !== undefined && matchingRowsPage(frame, page);
  const rows = rowsMatch ? page.response.rows : [];
  const rowByTarget = new Map(rows.map((row) => [targetKey(row), row]));
  const targets = new Map<string, VizSelection>();
  const wireShapeKey = (ref: string) => JSON.stringify(["shape", ref]);
  let validTargets = true;
  const shapes: GeometryRendererProps["shapes"] = frame.model.shapes.map(
    (shape, index) => {
      const key = targetKey(shape);
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
          targets.set(key, {
            ...base,
            kind: "item",
            itemRef: shape.itemRef,
            matchKey: shape.matchKey,
          });
        } else if (shape.groupRef !== null) {
          targets.set(key, {
            ...base,
            kind: "group",
            groupRef: shape.groupRef,
          });
        }
      }
      return {
        shapeRef:
          shape.shapeRef === undefined
            ? JSON.stringify(["flat", index])
            : wireShapeKey(shape.shapeRef),
        rowRef: key ?? "",
        geometry: shape.geometry,
        ...describe(shape, key ? rowByTarget.get(key) : undefined),
      };
    },
  );
  const relations: GeometryRendererProps["relations"] =
    frame.model.relationships.map((relation) => ({
      fromShapeRef: wireShapeKey(relation.fromShapeRef),
      toShapeRef: wireShapeKey(relation.toShapeRef),
      ...(relation.geometry === undefined
        ? {}
        : { geometry: relation.geometry }),
    }));
  return { shapes, relations, targets, rows, rowsMatch, validTargets };
}
