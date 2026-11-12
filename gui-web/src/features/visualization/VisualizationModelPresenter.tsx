import { useId, useLayoutEffect, useMemo, useRef } from "react";
import {
  Action,
  DataTable,
  DetailDisclosure,
  createChartRenderers,
  Notice,
  Progress,
  Readout,
  StatusBadge,
  Text,
  Heading,
} from "../../components";
import type {
  ItemRef,
  VizModel,
  VizRenderRequest,
  VizRow,
  VizRowsRequest,
} from "../../ipc";
import { adaptVizModel, selectionKey } from "./model-adapter";
import type {
  ModelResponse,
  ModelRowsPage,
  VizSelection,
} from "./model-adapter";
import { rawSourceSize, shapeLabels, vizMessage } from "./model-text";
import type { VizText } from "./model-text";

const defaultChartRenderer = createChartRenderers([]);
type ModelViewProps = {
  resolveRenderer?: ReturnType<typeof createChartRenderers> | undefined;
  frame: ModelResponse;
  page?: ModelRowsPage;
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
  const { frame, page, view, selected, text } = props;
  const visualizationRef = useRef<HTMLElement>(null);
  const headingId = useId();
  const ChartRenderer = (props.resolveRenderer ?? defaultChartRenderer)(
    frame.model.representation.format,
  );
  const rowsPending =
    props.pendingRowsRequest?.modelRef === frame.model.modelRef &&
    props.pendingRowsRequest.resultRevision === frame.resultRevision;
  const adapted = useMemo(
    () =>
      adaptVizModel(frame, page, (shape, row) =>
        shapeLabels(frame.model, shape, row, text),
      ),
    [frame, page, text],
  );
  const rowsContent =
    adapted.rowsMatch && page ? (
      <ModelRows
        page={page}
        model={frame.model}
        text={text}
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
      aria-labelledby={headingId}
    >
      <Heading level={2} id={headingId}>
        {text.message("storageVisualization")}
      </Heading>
      <div className="du-review-context">
        <StatusBadge
          tone={frame.completeness === "complete" ? "neutral" : "warning"}
        >
          {text.key(frame.completeness)}
        </StatusBadge>
        <StatusBadge>{text.key(frame.model.origin)}</StatusBadge>
        <StatusBadge
          tone={
            frame.model.staleness.state === "current" ? "neutral" : "warning"
          }
        >
          {text.key(frame.model.staleness.state)}
        </StatusBadge>
        <Text>{text.observedAt(frame.observedAt)}</Text>
      </div>
      {frame.model.shapes.length === 0 && (
        <Notice>{text.message("noDrawableItems")}</Notice>
      )}
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
            props.onFocusItem?.(target.kind === "item" ? target.itemRef : null);
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
          content: rows,
        }}
      />
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

export function ModelRows({
  page,
  model,
  text,
  onPage,
  onOpenGroup,
  returnTargetRef,
  title,
}: {
  page: ModelRowsPage;
  model: VizModel;
  text: VizText;
  onPage?: (request: VizRowsRequest) => void;
  onOpenGroup?: (
    groupRef: string,
    size: VizRow["size"],
    origin: HTMLElement,
    groupLabel: string,
    returnTarget: HTMLElement | null,
  ) => void;
  returnTargetRef?: { current: HTMLElement | null } | undefined;
  title?: string;
}) {
  const rowTitle = title ?? text.message("rowsFromGraph");
  const rowsRegion = useRef<HTMLElement>(null);
  const { request, response } = page;
  const next = request.start + response.rows.length;
  const hasOpenableGroups =
    onOpenGroup !== undefined &&
    response.rows.some((row) => row.groupRef !== null && row.itemRef === null);
  useLayoutEffect(() => {
    if (returnTargetRef) returnTargetRef.current = rowsRegion.current;
  }, [
    request.modelRef,
    request.resultRevision,
    request.start,
    response.rows,
    returnTargetRef,
  ]);
  return (
    <div
      ref={(element) => {
        rowsRegion.current = element;
        if (returnTargetRef) returnTargetRef.current = element;
      }}
      {...(returnTargetRef
        ? { role: "group", tabIndex: -1, "aria-label": rowTitle }
        : {})}
    >
      <DetailDisclosure
        title={rowTitle}
        key={`${request.modelRef}:${request.resultRevision}`}
      >
        <Text>
          {response.rows.length === 0
            ? text.message("noRowsInPage")
            : vizMessage(text, "rowRange", {
                start: String(request.start + 1),
                end: String(next),
                total: String(response.totalCount),
              })}
        </Text>
        <DataTable
          label={text.message("loadedGraphRows")}
          density="compact"
          columns={[
            text.message("name"),
            ...(hasOpenableGroups ? [text.message("actions")] : []),
            text.message("value"),
            text.message("ratio"),
            text.message("denominator"),
            text.message("classification"),
            text.message("flags"),
          ]}
          rows={response.rows}
          getRowKey={(row) => JSON.stringify([row.itemRef, row.groupRef])}
          renderCells={(row) => {
            let trigger: HTMLButtonElement | null = null;
            return [
              row.displayName,
              ...(hasOpenableGroups
                ? [
                    row.groupRef !== null &&
                    row.itemRef === null &&
                    onOpenGroup ? (
                      <Action
                        ref={(element) => {
                          trigger = element;
                        }}
                        onPress={() => {
                          if (trigger && row.groupRef !== null)
                            onOpenGroup(
                              row.groupRef,
                              row.size,
                              trigger,
                              row.displayName.trim() ||
                                text.message("otherItems"),
                              returnTargetRef?.current ?? rowsRegion.current,
                            );
                        }}
                        aria-label={vizMessage(text, "viewMembersOf", {
                          name:
                            row.displayName.trim() ||
                            text.message("otherItems"),
                        })}
                      >
                        {text.message("viewMembers")}
                      </Action>
                    ) : null,
                  ]
                : []),
              row.quantity ? text.quantity(row.quantity) : text.size(row.size),
              row.ratio ?? text.message("unavailableValue"),
              `${text.quantity(row.denominator.value)} · ${text.key(row.denominator.basis)}`,
              text.key(
                model.legend.find((entry) => entry.key === row.legendClassKey)
                  ?.labelKey ?? row.legendClassKey,
              ),
              row.displayFlags.map(text.key).join(" · "),
            ];
          }}
        />
        {onPage && (
          <div className="du-review-actions">
            <Action
              isDisabled={request.start === 0}
              onPress={() =>
                onPage({
                  ...request,
                  start: Math.max(0, request.start - request.count),
                })
              }
            >
              {text.message("previousRows")}
            </Action>
            <Action
              isDisabled={
                next >= response.totalCount || response.rows.length === 0
              }
              onPress={() => onPage({ ...request, start: next })}
            >
              {text.message("nextRows")}
            </Action>
          </div>
        )}
      </DetailDisclosure>
    </div>
  );
}

function ModelEvidence({
  frame,
  text,
}: {
  frame: ModelResponse;
  text: VizText;
}) {
  const { model } = frame;
  const totals = [
    [text.message("scopeTotal"), model.totals.scope],
    [text.message("drawnTotal"), model.totals.drawn],
    [text.message("groupedTotal"), model.totals.grouped],
    [text.message("filteredOutTotal"), model.totals.filteredOut],
  ] as const;
  return (
    <DetailDisclosure title={text.message("measurementBasisTotals")}>
      <Readout
        label={text.message("representation")}
        rows={[
          {
            label: text.message("format"),
            value: text.key(model.representation.format),
          },
          {
            label: text.message("measure"),
            value: text.key(model.representation.measure),
          },
          {
            label: text.message("sizeBasis"),
            value: text.key(model.representation.sizeBasis),
          },
          {
            label: text.message("denominator"),
            value: `${text.quantity(model.denominator.value)} · ${text.key(model.denominator.basis)}`,
          },
        ]}
      />
      <Readout
        label={text.message("independentTotals")}
        rows={totals.map(([label, total]) => ({
          label,
          value: text.quantity(total.quantity),
        }))}
      />
      <Readout
        label={text.message("sourceSizes")}
        rows={totals.map(([label, total]) => ({
          label,
          value:
            total.sourceSize === null
              ? text.message("noSourceSize")
              : rawSourceSize(total.sourceSize, text.message),
        }))}
      />
      <Readout
        label={text.message("coverage")}
        rows={[
          {
            label: text.message("groupedItems"),
            value: text.quantity(model.summary.groupedItems),
          },
          {
            label: text.message("omittedItems"),
            value: text.quantity(model.summary.omittedItems),
          },
          {
            label: text.message("unknownItems"),
            value: text.quantity(model.summary.unknownItems),
          },
        ]}
      />
      {model.summary.reasonKeys.map((key) => (
        <Notice key={key}>{text.key(key)}</Notice>
      ))}
      {model.staleness.reasonKey && (
        <Notice>{text.key(model.staleness.reasonKey)}</Notice>
      )}
    </DetailDisclosure>
  );
}
