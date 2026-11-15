import type { ItemRef } from "../../ipc";
import { sameItem } from "../../viz";
import type { ModelResponse, VizSelection } from "../../viz";

/** Evidence that an item of an earlier result is the same item, or has moved, in a later one. */
export type Reconciled = {
  from: ItemRef;
  to: ItemRef;
  outcome: "same" | "moved";
};

export function selectionPresent(
  selection: VizSelection,
  frame: ModelResponse,
) {
  if (
    selection.modelRef !== frame.model.modelRef ||
    selection.resultRevision !== frame.resultRevision
  )
    return false;
  const shapes = frame.model.shapes.filter((shape) =>
    selection.kind === "item"
      ? sameItem(shape.itemRef, selection.itemRef)
      : shape.groupRef === selection.groupRef,
  );
  return (
    shapes.length === 1 &&
    (selection.kind === "group" ||
      JSON.stringify(shapes[0]!.matchKey) ===
        JSON.stringify(selection.matchKey))
  );
}
export function transferItem(
  item: ItemRef | null,
  revision: string,
  evidence: readonly Reconciled[],
) {
  if (!item) return null;
  if (item.resultVersion === revision) return item;
  const matches = evidence.filter(
    (entry) =>
      sameItem(entry.from, item) &&
      entry.to.resultVersion === revision &&
      (entry.outcome === "same" || entry.outcome === "moved"),
  );
  return matches.length === 1 ? matches[0]!.to : null;
}
export function transferSelection(
  selection: VizSelection | null,
  frame: ModelResponse,
  evidence: readonly Reconciled[],
): VizSelection | null {
  if (!selection) return null;
  if (selection.kind === "group")
    return selection.modelRef === frame.model.modelRef &&
      selection.resultRevision === frame.resultRevision
      ? selection
      : null;
  if (
    selection.itemRef.resultVersion !== frame.resultRevision &&
    selection.matchKey.state !== "present"
  )
    return null;
  const item = transferItem(selection.itemRef, frame.resultRevision, evidence);
  if (!item) return null;
  const shapes = frame.model.shapes.filter((candidate) =>
    sameItem(candidate.itemRef, item),
  );
  if (shapes.length !== 1) return null;
  const shape = shapes[0]!;
  if (
    selection.resultRevision === frame.resultRevision &&
    JSON.stringify(shape.matchKey) !== JSON.stringify(selection.matchKey)
  )
    return null;
  return {
    kind: "item",
    itemRef: item,
    matchKey: shape.matchKey,
    modelRef: frame.model.modelRef,
    resultRevision: frame.resultRevision,
  };
}
