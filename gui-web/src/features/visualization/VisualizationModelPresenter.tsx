import { useMemo, useRef } from "react";
import type { ReactNode } from "react";
import {
  BarRenderer,
  createChartRenderers,
  HistogramRenderer,
  Notice,
  Progress,
  Text,
} from "../../components";
import type {
  ItemRef,
  VizRenderRequest,
  VizRow,
  VizRowsRequest,
  VizShape,
} from "../../ipc";
import {
  adaptVizModel,
  histogramRanges,
  legendEntries,
  selectionKey,
} from "../../viz";
import type { ModelResponse, ModelRowsPage, VizSelection } from "../../viz";
import { ModelEvidence, shapeLabels } from "../../viz";
import type { VizText } from "../../viz";
import { EmptyResult } from "./EmptyResult";
import { ModelStatus } from "./ModelStatus";
import { ModelRows } from "./ModelRows";
import { useChartOffset } from "./chart-area";
import { ModelLegend, useAutoPatterns, useClassChoice } from "./ModelLegend";
import type { LegendControls } from "./ModelLegend";

const defaultChartRenderer = createChartRenderers([
  ["bar", BarRenderer],
  ["histogram", HistogramRenderer],
]);
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
  /** The focused shape left the chart and no shape could take the focus. */
  onFocusTargetMissing?: () => void;
  onFocusMoved?: () => void;
  onFocusAvailability?: (available: boolean) => void;
  /**
   * Enter on a shape (README 8, 34). `kind` is the item kind the engine
   * supplied, or "folder" for an expanded folder; undefined when unknown.
   */
  onActivate?: (
    selection: VizSelection,
    shapeRef: string,
    kind: VizShape["itemKind"],
  ) => void;
  onClearSelection?: () => void;
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
  /** A control at the end of the status line (F06 README 43: the histogram axis). */
  statusAction?: ReactNode;
  /** The legend's controls; absent where a format has no legend (histogram). */
  legend?: LegendControls | undefined;
  /** Beside the chart from 720 CSS px of chart area, else under it (README 2). */
  legendSide?: "right" | "below" | undefined;
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
  // The histogram keeps the place of an empty interval, named by its row (README 43).
  const ranges = useMemo(
    () => histogramRanges(frame.model, adapted.rows, text),
    [frame.model, adapted.rows, text],
  );
  const entries = useMemo(
    () => legendEntries(frame.model, text),
    [frame.model, text],
  );
  const classes = useClassChoice(entries, adapted.shapes);
  const autoPatterns = useAutoPatterns();
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
      aria-label={text.message("storageVisualization")}
    >
      <div ref={contextRef}>
        <div className="du-review-context">
          <ModelStatus frame={frame} text={text} />
          {props.statusAction && (
            <div className="du-review-context__end">{props.statusAction}</div>
          )}
        </div>
        {frame.model.representation.format === "histogram" && (
          <div className="du-chart-axis-title">
            <Text kind="muted">
              {text.message(
                frame.model.representation.measure === "count"
                  ? "measureCount"
                  : "axisTitleSize",
              )}
            </Text>
          </div>
        )}
      </div>
      <div className="du-chart-and-legend" data-legend-side={props.legendSide}>
        <div ref={chartBox} className="du-chart-and-legend__chart">
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
            shapes={classes.shapes}
            relations={adapted.relations}
            ranges={ranges}
            patterns={patternsOn(props.legend, autoPatterns)}
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
            {...keyboardCallbacks(props, adapted)}
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
                      adapted.rowsMatch && page
                        ? page.response.totalCount
                        : null
                    }
                  />
                ) : (
                  rows
                ),
            }}
          />
        </div>
        <ModelLegend
          frame={frame}
          text={text}
          controls={props.legend}
          entries={entries}
          selected={classes.selected}
          onSelect={classes.choose}
          forced={autoPatterns}
        />
      </div>
      <ModelEvidence frame={frame} text={text} />
    </section>
  );
}

/** Patterns are drawn when the reader turned them on or the system asks for them. */
function patternsOn(legend: LegendControls | undefined, auto: boolean) {
  return auto || (legend?.patterns ?? false);
}

/** The renderer's focus and key callbacks the caller supplied (README 34). */
function keyboardCallbacks(
  props: ModelViewProps,
  adapted: ReturnType<typeof adaptVizModel>,
) {
  return {
    ...(props.onFocusAvailability
      ? { onFocusAvailability: props.onFocusAvailability }
      : {}),
    ...(props.onFocusTargetMissing
      ? { onFocusTargetMissing: props.onFocusTargetMissing }
      : {}),
    ...(props.onFocusMoved ? { onFocusMoved: props.onFocusMoved } : {}),
    ...(props.onClearSelection
      ? { onClearSelection: props.onClearSelection }
      : {}),
    onActivate: (rowRef: string, shapeRef: string, container: boolean) => {
      if (!eventShape(adapted, shapeRef, rowRef)) return;
      const target = adapted.targetsByShape.get(shapeRef);
      const kind =
        adapted.kindsByShape.get(shapeRef) ??
        (container ? "folder" : undefined);
      if (target) props.onActivate?.(target, shapeRef, kind);
    },
  };
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
