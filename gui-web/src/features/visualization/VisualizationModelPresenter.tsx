import { useMemo, useRef } from "react";
import { createChartRenderers, Notice, Progress } from "../../components";
import type {
  ItemRef,
  VizRenderRequest,
  VizRow,
  VizRowsRequest,
} from "../../ipc";
import { adaptVizModel, selectionKey } from "../../viz";
import type { ModelResponse, ModelRowsPage, VizSelection } from "../../viz";
import { ModelEvidence, shapeLabels } from "../../viz";
import type { VizText } from "../../viz";
import { EmptyResult } from "./EmptyResult";
import { ModelStatus } from "./ModelStatus";
import { ModelRows } from "./ModelRows";
import { useChartOffset } from "./chart-area";

const defaultChartRenderer = createChartRenderers([]);
type ModelViewProps = {
  resolveRenderer?: ReturnType<typeof createChartRenderers> | undefined;
  frame: ModelResponse;
  page?: ModelRowsPage;
  /** Rows of the expanded folders of `frame`, for the names of nested shapes. */
  folderRows?: readonly VizRow[];
  /** Supplied only while this exact row request is in flight. */
  pendingRowsRequest?: VizRowsRequest;
  view: Pick<VizRenderRequest, "viewport" | "visibleWindow">;
  selected: VizSelection | null;
  onSelect: (selection: VizSelection, shapeRef: string) => void;
  onFocusedGroupShape?: (
    selection: VizSelection | null,
    shapeRef: string,
    target: HTMLElement | null,
    groupLabel?: string,
    returnTarget?: HTMLElement | null,
  ) => void;
  focus?: ItemRef | null;
  onFocusItem?: (item: ItemRef | null) => void;
  onFocusTargetMissing?: () => void;
  onFocusAvailability?: (available: boolean) => void;
  focusFirstRequest?: number;
  onPage?: (request: VizRowsRequest) => void;
  onOpenGroup?: (
    groupRef: string,
    size: VizRow["size"],
    origin: HTMLElement,
    groupLabel: string,
    returnTarget: HTMLElement | null,
  ) => void;
  returnTargetRef?: { current: HTMLElement | null } | undefined;
  rowsReturnTargetRef?: { current: HTMLElement | null } | undefined;
  text: VizText;
};

/** A committed model only. Request ordering and last-valid-model retention belong to its controller. */
export function VisualizationModelPresenter(props: ModelViewProps) {
  const { frame, page, view, selected, text, folderRows } = props;
  const visualizationRef = useRef<HTMLElement>(null);
  const chartBox = useRef<HTMLDivElement>(null);
  const contextRef = useRef<HTMLDivElement>(null);
  useChartOffset(chartBox, contextRef);
  const ChartRenderer = (props.resolveRenderer ?? defaultChartRenderer)(
    frame.model.representation.format,
  );
  const rowsPending =
    props.pendingRowsRequest?.modelRef === frame.model.modelRef &&
    props.pendingRowsRequest.resultRevision === frame.resultRevision;
  const adapted = useMemo(
    () =>
      adaptVizModel(
        frame,
        page,
        (shape, row, place) =>
          shapeLabels(frame.model, shape, row, text, place),
        folderRows,
      ),
    [frame, page, text, folderRows],
  );
  const rowsContent =
    adapted.rowsMatch && page ? (
      <ModelRows
        page={page}
        model={frame.model}
        text={text}
        expanded
        returnTargetRef={props.rowsReturnTargetRef}
        {...(props.onOpenGroup ? { onOpenGroup: props.onOpenGroup } : {})}
        {...(props.onPage ? { onPage: props.onPage } : {})}
      />
    ) : rowsPending ? null : (
      <Notice>{text.message("rowsNotRetrieved")}</Notice>
    );
  const rows = (
    <>
      {rowsPending && <Progress label={text.message("loadingGraphRows")} />}
      {rowsContent}
    </>
  );
  if (!adapted.validTargets)
    return <Notice tone="error">{text.message("invalidReferences")}</Notice>;
  return (
    <section
      ref={(element) => {
        const previous = visualizationRef.current;
        visualizationRef.current = element;
        if (element && props.returnTargetRef)
          props.returnTargetRef.current = element;
        else if (props.returnTargetRef?.current === previous)
          props.returnTargetRef.current = null;
      }}
      className="du-review-stack"
      tabIndex={-1}
      aria-label={text.message("storageVisualization")}
    >
      <div ref={contextRef} className="du-review-context">
        <ModelStatus frame={frame} text={text} />
      </div>
      <div ref={chartBox}>
        <ChartRenderer
          modelRef={frame.model.modelRef}
          resultRevision={frame.resultRevision}
          label={text.message("spaceDistribution")}
          selectedText={text.message("selected")}
          fallbackNotice={text.message("graphFallback")}
          missingFallbackNotice={text.message("graphRowsUnavailable")}
          width={view.viewport.width}
          height={view.viewport.height}
          viewport={view.visibleWindow}
          shapes={adapted.shapes}
          relations={adapted.relations}
          selectedRowRef={selected ? selectionKey(selected, frame) : null}
          focusedRowRef={
            props.focus && props.focus.resultVersion === frame.resultRevision
              ? JSON.stringify([
                  "item",
                  props.focus.resultVersion,
                  props.focus.itemNo,
                ])
              : null
          }
          {...(props.onFocusAvailability
            ? { onFocusAvailability: props.onFocusAvailability }
            : {})}
          {...(props.focusFirstRequest === undefined
            ? {}
            : { focusFirstRequest: props.focusFirstRequest })}
          {...(props.onFocusTargetMissing
            ? { onFocusTargetMissing: props.onFocusTargetMissing }
            : {})}
          onShapeFocus={(rowRef, shapeRef, control) => {
            const shape = eventShape(adapted, shapeRef, rowRef);
            const target = shape
              ? (adapted.targetsByShape.get(shapeRef) ?? null)
              : null;
            if (target)
              props.onFocusItem?.(
                target.kind === "item" ? target.itemRef : null,
              );
            props.onFocusedGroupShape?.(
              target,
              shapeRef,
              control && visualizationRef.current?.contains(control)
                ? control
                : null,
              shape?.accessibleLabel,
              visualizationRef.current,
            );
          }}
          onSelect={(rowRef, shapeRef) => {
            if (!eventShape(adapted, shapeRef, rowRef)) return;
            const selection = adapted.targetsByShape.get(shapeRef);
            if (selection) props.onSelect(selection, shapeRef);
          }}
          fallback={{
            modelRef: frame.model.modelRef,
            resultRevision: frame.resultRevision,
            content:
              frame.model.shapes.length === 0 ? (
                <EmptyResult
                  frame={frame}
                  text={text}
                  rowsTotal={
                    adapted.rowsMatch && page ? page.response.totalCount : null
                  }
                />
              ) : (
                rows
              ),
          }}
        />
      </div>
      <ModelEvidence frame={frame} text={text} />
    </section>
  );
}

/** Matches a renderer event to one shape and its source row in this model. */
function eventShape(
  adapted: ReturnType<typeof adaptVizModel>,
  shapeRef: string,
  rowRef: string,
) {
  const matches = adapted.shapes.filter((shape) => shape.shapeRef === shapeRef);
  const shape = matches[0];
  return matches.length === 1 && shape?.rowRef === rowRef ? shape : null;
}

export { ModelRows };
