import { useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { Action, LiveStatus, Notice } from "../../components";
import { VisualizationModelPresenter } from "./VisualizationModelPresenter";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { createChartRenderers } from "../../components";
import type { VizText } from "../../viz";
import { matchingRowsPage } from "../../viz";
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
import { VisualizationStatus } from "./VisualizationStatus";
import { RowsRetry } from "./RetryNotices";
import { SeparateModelTable, useFirstRows } from "./NewerModelTable";

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

function hasGraphTargets(
  available: boolean,
  graph: VisualizationSnapshot["graph"],
) {
  return available && !!graph?.frame.model.shapes.length;
}
