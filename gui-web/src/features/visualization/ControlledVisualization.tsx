import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { LiveStatus, Notice, Text, Toast } from "../../components";
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
  viewTitle,
} from "./group-navigation";
import { VisualizationStatus } from "./VisualizationStatus";
import { UnavailableView } from "./UnavailableView";
import type { BackAction } from "./UnavailablePanel";
import { RowsRetry } from "./RetryNotices";
import { SeparateModelTable, useFirstRows } from "./NewerModelTable";
import { useFolderRows } from "./folder-rows";

/** Injectable entry point. Visual design and product assembly remain with the host. */
export function ControlledVisualization({
  controller,
  text,
  onApplyRevision,
  resolveRenderer,
  autoLoadRows,
  groupController,
  backAction,
  sizeBasis,
}: {
  controller: VisualizationController;
  text: VizText;
  onApplyRevision?: (revision: string) => void;
  resolveRenderer?: ReturnType<typeof createChartRenderers>;
  autoLoadRows?: boolean;
  groupController?: GroupController;
  backAction?: BackAction;
  /** The size basis of the current request, named when a chart lacks it. */
  sizeBasis?: string;
}) {
  const state = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
  );
  const [graphTargets, setGraphTargets] = useState(false);
  // Said once in the status region: where the focus went, or why Enter did
  // nothing. A new node each time, so the same words are said again.
  const [announcement, setAnnouncement] = useState({ message: "", count: 0 });
  const announce = (message: string) =>
    setAnnouncement((current) => ({ message, count: current.count + 1 }));
  const selectionLostNow = useLostInThisUpdate(state.previousSelection);
  const [openGroupRequest, setOpenGroupRequest] = useState(0);
  const visualizationTargetRef = useRef<HTMLElement | null>(null);
  const graphRowsTargetRef = useRef<HTMLElement | null>(null);
  const tableTargetRef = useRef<HTMLElement | null>(null);
  const [groupOrigin, setGroupOrigin] = useState<GroupOrigin | null>(null);
  const focusedGroupShape =
    useRef<ReturnType<typeof rememberFocusedGroupShape>>(null);
  const graph = state.graph;
  const table = state.table;
  // The panel takes the chart's place; the previous model stays in the controller.
  const unavailable = unavailableResponse(state);
  const targetsShown = hasGraphTargets(graphTargets, graph, unavailable);
  const tableRowsFrame = getMatchingTableFrame(table, state.tablePage);
  useFirstRows(controller, state, autoLoadRows === true);
  const folderRows = useFolderRows(controller.folderRows, graph);
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
        graphTargets={targetsShown}
        {...(onApplyRevision ? { onApplyRevision } : {})}
      />
      {unavailable && (
        <UnavailableView
          reasonKey={unavailable.reasonKey}
          completeness={unavailable.completeness}
          sizeBasis={sizeBasis}
          controller={controller}
          state={state}
          text={text}
          backAction={backAction}
        />
      )}
      {graph && table && !currentTable && (
        <Notice>
          {text.message("retainedGraph")}{" "}
          {text.observedAt(graph.frame.observedAt)}
        </Notice>
      )}
      {!unavailable && graph && (
        <VisualizationModelPresenter
          frame={graph.frame}
          resolveRenderer={resolveRenderer}
          view={graph.view}
          selected={state.selection}
          focus={state.focus}
          onFocusItem={(item) => {
            controller.focus(item);
          }}
          // The focused shape left the chart: a shape nearby took the focus,
          // or none is left and the screen title takes it (README 34).
          onFocusMoved={() => {
            if (!selectionLostNow) announce(text.message("focusMovedNearby"));
          }}
          onFocusTargetMissing={() => {
            const title = viewTitle(visualizationTargetRef.current);
            if (!title) return;
            title.focus({ preventScroll: true });
            if (!selectionLostNow) announce(text.message("focusMovedTitle"));
          }}
          onFocusAvailability={setGraphTargets}
          onClearSelection={() => controller.select(null)}
          onActivate={(selection, shapeRef, container) => {
            if (selection.kind === "group") {
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
              );
              setOpenGroupRequest((value) => value + 1);
            } else
              announce(
                text.message(
                  container ? "folderExpandUnavailable" : "itemOpenUnavailable",
                ),
              );
          }}
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
          folderRows={folderRows}
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
          openRequest={openGroupRequest}
          text={text}
        />
      )}
      <LiveStatus>
        {announcement.message && (
          <Text key={announcement.count}>{announcement.message}</Text>
        )}
      </LiveStatus>
      <Toast message={announcement.message} version={announcement.count} />
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

/** A selection lost in this update is said by its own notice, and alone (README 34). */
function useLostInThisUpdate(
  previous: VisualizationSnapshot["previousSelection"],
) {
  const seen = useRef(previous);
  useEffect(() => {
    seen.current = previous;
  });
  return previous !== null && previous !== seen.current;
}

function unavailableResponse(state: VisualizationSnapshot) {
  return state.phase === "unavailable" && state.response?.kind === "unavailable"
    ? state.response
    : null;
}

function hasGraphTargets(
  available: boolean,
  graph: VisualizationSnapshot["graph"],
  unavailable: unknown,
) {
  return available && !unavailable && !!graph?.frame.model.shapes.length;
}
