import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  Action,
  Heading,
  LiveStatus,
  Notice,
  Progress,
  Text,
} from "../../components";
import {
  ModelRows,
  VisualizationModelPresenter,
} from "./VisualizationModelPresenter";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { VizRow, VizRowsRequest } from "../../ipc";
import type { createChartRenderers } from "../../components";
import type { VizText } from "./model-text";
import { matchingRowsPage } from "./model-adapter";
import { visualizationFormatName, visualizationReason } from "./messages";
import { GroupView } from "./GroupView";
import type { GroupController } from "./group-controller";
import type { GroupOrigin } from "./group-navigation";
import {
  createGroupRowOpener,
  getGroupReturnTargetRef,
  getMatchingTableFrame,
  getGroupViewFrame,
  isCurrentTable,
  rememberFocusedGroupShape,
  selectVisualizationGroup,
} from "./group-navigation";

/** Injectable entry point. Visual design and product assembly remain with the host. */
export function ControlledVisualization({
  controller,
  text,
  onApplyRevision,
  resolveRenderer,
  autoLoadRows,
  groupController,
}: {
  controller: VisualizationController;
  text: VizText;
  onApplyRevision?: (revision: string) => void;
  resolveRenderer?: ReturnType<typeof createChartRenderers>;
  autoLoadRows?: boolean;
  groupController?: GroupController;
}) {
  const state = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
  );
  const [focusLost, setFocusLost] = useState(false);
  const [graphTargets, setGraphTargets] = useState(false);
  const [focusRequest, setFocusRequest] = useState(0);
  const focusNotice = useRef<HTMLDivElement>(null);
  const visualizationTargetRef = useRef<HTMLElement | null>(null);
  const graphRowsTargetRef = useRef<HTMLElement | null>(null);
  const tableTargetRef = useRef<HTMLElement | null>(null);
  const [groupOrigin, setGroupOrigin] = useState<GroupOrigin | null>(null);
  const focusedGroupShape =
    useRef<ReturnType<typeof rememberFocusedGroupShape>>(null);
  useLayoutEffect(() => {
    if (focusLost && document.activeElement === document.body)
      focusNotice.current?.focus({ preventScroll: true });
  }, [focusLost]);
  const graph = state.graph;
  const table = state.table;
  const tableRowsFrame = getMatchingTableFrame(table, state.tablePage);
  useFirstRows(controller, state, autoLoadRows === true);
  const currentTable = isCurrentTable(graph, table);
  const graphRowsFrame =
    graph && state.graphPage && matchingRowsPage(graph.frame, state.graphPage)
      ? graph.frame
      : null;
  const groupViewFrame = getGroupViewFrame(groupOrigin, graph, tableRowsFrame);
  const groupReturnTargetRef = getGroupReturnTargetRef(
    groupOrigin,
    graph,
    table,
    graphRowsTargetRef,
    visualizationTargetRef,
    tableTargetRef,
  );
  const openGroupFromGraphRows = createGroupRowOpener(
    groupController,
    graphRowsFrame,
    setGroupOrigin,
  );
  const openGroupFromTableRows = createGroupRowOpener(
    groupController,
    tableRowsFrame,
    setGroupOrigin,
  );
  return (
    <>
      <VisualizationStatus
        state={state}
        controller={controller}
        text={text}
        graphTargets={hasGraphTargets(graphTargets, graph)}
        {...(onApplyRevision ? { onApplyRevision } : {})}
      />
      {graph && table && !currentTable && (
        <Notice>
          {text.message("retainedGraph")}{" "}
          {text.observedAt(graph.frame.observedAt)}
        </Notice>
      )}
      {graph && (
        <VisualizationModelPresenter
          frame={graph.frame}
          resolveRenderer={resolveRenderer}
          view={graph.view}
          selected={state.selection}
          focus={state.focus}
          onFocusItem={(item) => {
            setFocusLost(false);
            controller.focus(item);
          }}
          onFocusTargetMissing={() => setFocusLost(true)}
          onFocusAvailability={setGraphTargets}
          focusFirstRequest={focusRequest}
          onFocusedGroupShape={(
            selection,
            shapeRef,
            target,
            groupLabel,
            viewTarget,
          ) =>
            (focusedGroupShape.current = rememberFocusedGroupShape(
              graph.frame,
              selection,
              shapeRef,
              target,
              groupLabel,
              viewTarget,
            ))
          }
          onSelect={(selection, shapeRef) =>
            selectVisualizationGroup(
              selection,
              shapeRef,
              graph.frame,
              controller,
              focusedGroupShape.current,
              setGroupOrigin,
              () => {
                focusedGroupShape.current = null;
              },
            )
          }
          {...(openGroupFromGraphRows
            ? { onOpenGroup: openGroupFromGraphRows }
            : {})}
          returnTargetRef={visualizationTargetRef}
          rowsReturnTargetRef={graphRowsTargetRef}
          text={text}
          {...(state.graphPage ? { page: state.graphPage } : {})}
          {...(state.pendingRows && currentTable
            ? { pendingRowsRequest: state.pendingRows }
            : {})}
          {...(currentTable
            ? {
                onPage: (request) => {
                  void controller.rows(request);
                },
              }
            : {})}
        />
      )}
      {groupController && (
        <GroupView
          controller={groupController}
          frame={groupViewFrame}
          selection={state.selection}
          origin={groupOrigin}
          returnTargetRef={groupReturnTargetRef}
          text={text}
        />
      )}
      {focusLost && (
        <div>
          <LiveStatus ref={focusNotice} focusTarget>
            {text.message("focusLost")}
          </LiveStatus>
          {hasGraphTargets(graphTargets, graph) && (
            <Action onPress={() => setFocusRequest((value) => value + 1)}>
              {text.message("focusGraphAction")}
            </Action>
          )}
        </div>
      )}
      <RowsRetry
        controller={controller}
        state={state}
        text={text}
        newer={!!graph && !currentTable}
      />
      <SeparateModelTable
        state={state}
        controller={controller}
        text={text}
        currentTable={currentTable}
        {...(openGroupFromTableRows
          ? { onOpenGroup: openGroupFromTableRows }
          : {})}
        returnTargetRef={tableTargetRef}
      />
    </>
  );
}

function useFirstRows(
  controller: VisualizationController,
  state: VisualizationSnapshot,
  autoLoadRows: boolean,
) {
  const table = state.table;
  useEffect(() => {
    if (
      !autoLoadRows ||
      state.phase !== "model" ||
      !table ||
      state.tablePage ||
      state.pendingRows ||
      state.failedRows
    )
      return;
    void controller.rows({
      modelRef: table.model.modelRef,
      resultRevision: table.resultRevision,
      start: 0,
      count: 100,
    });
  }, [
    autoLoadRows,
    controller,
    state.phase,
    state.tablePage,
    state.pendingRows,
    state.failedRows,
    table,
  ]);
}

function SeparateModelTable({
  state,
  controller,
  text,
  currentTable,
  onOpenGroup,
  returnTargetRef,
}: {
  state: VisualizationSnapshot;
  controller: VisualizationController;
  text: VizText;
  currentTable: boolean;
  onOpenGroup?: (
    groupRef: string,
    size: VizRow["size"],
    origin: HTMLElement,
    groupLabel: string,
    returnTarget: HTMLElement | null,
  ) => void;
  returnTargetRef: { current: HTMLElement | null };
}) {
  const graph = state.graph;
  const table = state.table;
  const sectionRef = useRef<HTMLElement>(null);
  const headingId = useId();
  return (
    <>
      {table && (!graph || !currentTable) && (
        <section
          ref={(element) => {
            const previous = sectionRef.current;
            sectionRef.current = element;
            if (element) returnTargetRef.current = element;
            else if (returnTargetRef.current === previous)
              returnTargetRef.current = null;
          }}
          className="du-review-stack"
          tabIndex={-1}
          aria-labelledby={headingId}
        >
          <Heading level={2} id={headingId}>
            {text.message("newerRegion")}
          </Heading>
          <Text>
            {text.message("newTableObservation")}{" "}
            {text.observedAt(table.observedAt)}
          </Text>
          {graph && (
            <Text>
              {text.message(
                graph.frame.resultRevision === table.resultRevision
                  ? "sameRevision"
                  : "differentRevision",
              )}
            </Text>
          )}
          {state.pendingRows && (
            <Progress label={text.message("loadingNewer")} />
          )}
          {!state.tablePage && !state.pendingRows && !state.failedRows && (
            <Action
              onPress={() => {
                void controller.rows({
                  modelRef: table.model.modelRef,
                  resultRevision: table.resultRevision,
                  start: 0,
                  count: 100,
                });
              }}
            >
              {text.message("loadNewer")}
            </Action>
          )}
          {state.tablePage && (
            <ModelRows
              page={state.tablePage}
              title={text.message("newerTitle")}
              model={table.model}
              text={text}
              onPage={(request) => {
                void controller.rows(request);
              }}
              {...(onOpenGroup ? { onOpenGroup } : {})}
            />
          )}
        </section>
      )}
    </>
  );
}

function VisualizationStatus({
  state,
  controller,
  text,
  onApplyRevision,
  graphTargets,
}: {
  state: VisualizationSnapshot;
  controller: VisualizationController;
  text: VizText;
  onApplyRevision?: (revision: string) => void;
  graphTargets: boolean;
}) {
  return (
    <>
      {state.phase === "loading" && (
        <Progress label={text.message("loading")} />
      )}
      {state.phase === "job" && <Notice>{text.message("working")}</Notice>}
      {state.phase === "cancelled" && (
        <Notice>{text.message("cancelled")}</Notice>
      )}
      <RenderRetry state={state} controller={controller} text={text} />
      {state.revisionCheck && (
        <Notice>
          {text.message(
            state.revisionCheck === "stale"
              ? "revisionStale"
              : "revisionUnverified",
          )}
        </Notice>
      )}
      {state.phase === "unavailable" &&
        state.response?.kind === "unavailable" && (
          <Notice>
            {text.message("unavailable")}{" "}
            {visualizationReason(state.response.reasonKey, text.message)}.{" "}
            {text.message("formats")}{" "}
            {state.response.availableFormats
              .map((format) => visualizationFormatName(format, text.message))
              .join(", ") || text.message("noFormats")}
            .
          </Notice>
        )}
      {state.previousSelection && (
        <Notice>
          <Text>{text.message("selectionLost")}</Text>
          {state.previousSelectionEvidence?.name && (
            <Text>
              {text.message("previousName")}{" "}
              {state.previousSelectionEvidence.name}
            </Text>
          )}
          {state.previousSelectionEvidence && (
            <Text>
              {text.message("previousBasis")}{" "}
              {text.observedAt(state.previousSelectionEvidence.observedAt)} ·{" "}
              {text.key(state.previousSelectionEvidence.origin)} ·{" "}
              {text.message("previousMeasure")}{" "}
              {text.key(state.previousSelectionEvidence.sizeBasis)}
            </Text>
          )}
          <Text>{text.message("correspondenceUnknown")}</Text>
          {!graphTargets && <Text>{text.message("noGraphTargets")}</Text>}
        </Notice>
      )}
      {state.offeredRevision && (
        <Notice>
          {text.message("newerAvailable")}{" "}
          {onApplyRevision && (
            <Action
              onPress={() => {
                if (state.offeredRevision)
                  onApplyRevision(state.offeredRevision);
              }}
            >
              {text.message("reviewNewer")}
            </Action>
          )}
        </Notice>
      )}
    </>
  );
}

function RenderRetry({
  state,
  controller,
  text,
}: {
  state: VisualizationSnapshot;
  controller: VisualizationController;
  text: VizText;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const status = useRef<HTMLDivElement>(null);
  const busy = useRef(false);
  const [retained, setRetained] = useState(false);
  const [settled, setSettled] = useState(false);
  useLayoutEffect(() => {
    if (!settled) return;
    const phase = controller.getSnapshot().phase;
    if (phase !== "error" && document.activeElement === button.current)
      status.current?.focus({ preventScroll: true });
    busy.current = false;
    setRetained(false);
    setSettled(false);
  }, [controller, settled]);
  function retry() {
    if (busy.current) return;
    busy.current = true;
    setRetained(true);
    void controller.retry().finally(() => setSettled(true));
  }
  return (
    <>
      {(state.phase === "error" || retained) && (
        <Notice tone="error">
          {text.message("renderFailed")}{" "}
          <Action ref={button} aria-disabled={busy.current} onPress={retry}>
            {text.message(busy.current ? "retryingRender" : "retryRender")}
          </Action>
        </Notice>
      )}
      <LiveStatus ref={status} focusTarget>
        {state.phase === "model"
          ? text.message(
              state.graph?.frame === state.table
                ? "graphUpdated"
                : "newerRowsStatus",
            )
          : state.phase === "unavailable"
            ? text.message("unavailable")
            : state.phase === "job"
              ? text.message("working")
              : state.phase === "cancelled"
                ? text.message("cancelled")
                : ""}
      </LiveStatus>
    </>
  );
}

function RowsRetry({
  controller,
  state,
  text,
  newer,
}: {
  controller: VisualizationController;
  state: VisualizationSnapshot;
  text: VizText;
  newer: boolean;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const status = useRef<HTMLDivElement>(null);
  const busy = useRef(false);
  const [retained, setRetained] = useState<VizRowsRequest | null>(null);
  const [settled, setSettled] = useState(false);
  const request = state.failedRows ?? retained;
  useLayoutEffect(() => {
    if (!settled || !retained) return;
    const table = controller.getSnapshot().table;
    if (
      !controller.getSnapshot().failedRows &&
      table?.model.modelRef === retained.modelRef &&
      table.resultRevision === retained.resultRevision &&
      document.activeElement === button.current
    )
      status.current?.focus({ preventScroll: true });
    busy.current = false;
    setRetained(null);
    setSettled(false);
  }, [controller, retained, settled]);
  function retry() {
    if (!request || busy.current) return;
    busy.current = true;
    setRetained(request);
    void controller.rows(request).finally(() => setSettled(true));
  }
  return (
    <>
      {request && (
        <Notice tone="error">
          {text.message("rowsFailed")}{" "}
          <Action ref={button} aria-disabled={busy.current} onPress={retry}>
            {text.message(busy.current ? "retryingRows" : "retryRows")}
          </Action>
        </Notice>
      )}
      {state.table && (
        <LiveStatus ref={status} focusTarget>
          {text.message(newer ? "newerRowsStatus" : "graphRowsStatus")}
        </LiveStatus>
      )}
    </>
  );
}

function hasGraphTargets(
  available: boolean,
  graph: VisualizationSnapshot["graph"],
) {
  return available && !!graph?.frame.model.shapes.length;
}
