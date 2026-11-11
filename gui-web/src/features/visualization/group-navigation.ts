import type { Dispatch, SetStateAction } from "react";
import type { VizRow } from "../../ipc";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { GroupController } from "./group-controller";
import { matchingRowsPage } from "./model-adapter";
import type { ModelResponse, VizSelection } from "./model-adapter";

export type GroupOrigin = {
  element: HTMLElement;
  modelRef: string;
  resultRevision: string;
  observedAt: ModelResponse["observedAt"];
  completeness: ModelResponse["completeness"];
  groupRef: string;
  groupLabel?: string;
  returnTarget?: HTMLElement | null;
} & ({ kind: "shape"; shapeRef: string } | { kind: "row" });
type FocusedGroupShape = Extract<GroupOrigin, { kind: "shape" }>;

export function isCurrentTable(
  graph: VisualizationSnapshot["graph"],
  table: VisualizationSnapshot["table"],
) {
  return Boolean(
    graph &&
    table &&
    table.model.modelRef === graph.frame.model.modelRef &&
    table.resultRevision === graph.frame.resultRevision &&
    table.observedAt === graph.frame.observedAt &&
    table.completeness === graph.frame.completeness,
  );
}

export function getTableOnlyFrame(
  _graph: VisualizationSnapshot["graph"],
  table: VisualizationSnapshot["table"],
  state: VisualizationSnapshot,
): ModelResponse | null {
  const page = state.tablePage;
  if (!table || !page || !matchingRowsPage(table, page)) return null;
  return table;
}

export function getGroupRowFrame(tableFrame: ModelResponse | null) {
  return tableFrame;
}

export function getGroupViewFrame(
  origin: GroupOrigin | null,
  graph: VisualizationSnapshot["graph"],
  tableFrame: ModelResponse | null,
) {
  const candidates = [graph?.frame ?? null, tableFrame];
  if (!origin) return candidates[0] ?? candidates[1] ?? null;
  return (
    candidates.find(
      (frame) =>
        frame?.model.modelRef === origin.modelRef &&
        frame.resultRevision === origin.resultRevision &&
        frame.observedAt === origin.observedAt &&
        frame.completeness === origin.completeness,
    ) ?? null
  );
}

export function getGroupReturnTargetRef(
  origin: GroupOrigin | null,
  graph: VisualizationSnapshot["graph"],
  table: VisualizationSnapshot["table"],
  graphRowsTargetRef: { current: HTMLElement | null },
  visualizationTargetRef: { current: HTMLElement | null },
  tableTargetRef: { current: HTMLElement | null },
) {
  if (
    origin?.kind === "row" &&
    graph?.frame.model.modelRef === origin.modelRef &&
    graph.frame.resultRevision === origin.resultRevision &&
    graph.frame.observedAt === origin.observedAt &&
    graph.frame.completeness === origin.completeness
  )
    return graphRowsTargetRef;
  if (
    origin?.kind === "row" &&
    table?.model.modelRef === origin.modelRef &&
    table.resultRevision === origin.resultRevision &&
    table.observedAt === origin.observedAt &&
    table.completeness === origin.completeness
  )
    return tableTargetRef;
  return graph ? visualizationTargetRef : tableTargetRef;
}

export function createGroupRowOpener(
  groupController: GroupController | undefined,
  frame: ModelResponse | null,
  setGroupOrigin: Dispatch<SetStateAction<GroupOrigin | null>>,
) {
  if (!groupController || !frame) return undefined;
  return (
    groupRef: string,
    size: VizRow["size"],
    origin: HTMLElement,
    groupLabel: string,
    returnTarget: HTMLElement | null,
  ) => {
    setGroupOrigin({
      element: origin,
      kind: "row",
      modelRef: frame.model.modelRef,
      resultRevision: frame.resultRevision,
      observedAt: frame.observedAt,
      completeness: frame.completeness,
      groupRef,
      ...(groupLabel.trim() ? { groupLabel } : {}),
      returnTarget,
    });
    void groupController.open(
      {
        modelRef: frame.model.modelRef,
        resultRevision: frame.resultRevision,
        observedAt: frame.observedAt,
        completeness: frame.completeness,
      },
      { groupRef, shape: { groupRef, size } },
    );
  };
}

export function selectVisualizationGroup(
  selection: VizSelection,
  shapeRef: string,
  frame: ModelResponse,
  controller: VisualizationController,
  focusedGroupShape: FocusedGroupShape | null,
  setGroupOrigin: Dispatch<SetStateAction<GroupOrigin | null>>,
  clearFocusedGroupShape: () => void,
) {
  if (selection.kind === "group") {
    const verified =
      focusedGroupShape?.shapeRef === shapeRef &&
      focusedGroupShape.modelRef === frame.model.modelRef &&
      focusedGroupShape.resultRevision === frame.resultRevision &&
      focusedGroupShape.groupRef === selection.groupRef &&
      focusedGroupShape.element.isConnected &&
      document.activeElement === focusedGroupShape.element;
    setGroupOrigin(
      verified && focusedGroupShape
        ? { ...focusedGroupShape, kind: "shape" }
        : null,
    );
  }
  clearFocusedGroupShape();
  controller.select(selection);
}

export function rememberFocusedGroupShape(
  frame: ModelResponse,
  selection: VizSelection | null,
  shapeRef: string,
  target: HTMLElement | null,
  groupLabel?: string,
  returnTarget?: HTMLElement | null,
): FocusedGroupShape | null {
  if (
    !target ||
    !target.isConnected ||
    document.activeElement !== target ||
    selection?.kind !== "group" ||
    selection.modelRef !== frame.model.modelRef ||
    selection.resultRevision !== frame.resultRevision
  )
    return null;
  return {
    element: target,
    kind: "shape",
    shapeRef,
    modelRef: frame.model.modelRef,
    resultRevision: frame.resultRevision,
    observedAt: frame.observedAt,
    completeness: frame.completeness,
    groupRef: selection.groupRef,
    ...(groupLabel?.trim() ? { groupLabel } : {}),
    ...(returnTarget === undefined ? {} : { returnTarget }),
  };
}
