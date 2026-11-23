import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import type { LegendControls, TypeNarrowing } from "./ModelLegend";
import { LiveStatus, Notice, Text, Toast } from "../../components";
import type { AnalysisClient } from "../../ipc";
import { VisualizationModelPresenter } from "./VisualizationModelPresenter";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { createChartRenderers } from "../../components";
import type { ModelResponse, VizSelection, VizText } from "../../viz";
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
import { intervalListing, intervalRow } from "./interval-list";
import type { IntervalListing } from "./interval-list";

/** Injectable entry point. Visual design and product assembly remain with the host. */
export function ControlledVisualization({
  client,
  controller,
  text,
  onApplyRevision,
  resolveRenderer,
  autoLoadRows,
  groupController,
  backAction,
  formatAction,
  onAvailableFormats,
  sizeBasis,
  statusAction,
  legend,
  narrowing,
  legendSide,
  onOpenList,
}: {
  client?: AnalysisClient | undefined;
  controller: VisualizationController;
  text: VizText;
  onApplyRevision?: (revision: string) => void;
  resolveRenderer?: ReturnType<typeof createChartRenderers>;
  autoLoadRows?: boolean;
  groupController?: GroupController;
  backAction?: BackAction;
  /** Offered first in the unavailable notice: a chart that can be drawn. */
  formatAction?: BackAction;
  /** Told the formats that can be drawn whenever the engine cannot draw the asked one. */
  onAvailableFormats?: (formats: readonly string[]) => void;
  /** The size basis of the current request, named when a chart lacks it. */
  sizeBasis?: string;
  /** A control at the end of the chart's status line, such as the histogram axis. */
  statusAction?: ReactNode;
  /** The legend's controls, where the format has a legend. */
  legend?: LegendControls | undefined;
  /** The type the legend narrows to (README 46), kept by the screen. */
  narrowing?: TypeNarrowing | undefined;
  legendSide?: "right" | "below" | undefined;
  /**
   * Opens a histogram interval's file list in the chart's place (F06 README 12,
   * 52); without it an interval opens its members beside the chart.
   */
  onOpenList?: ((listing: IntervalListing) => void) | undefined;
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
  const shownGroups = groupsBesideChart(groupController, unavailable);
  const onAvailableRef = useRef(onAvailableFormats);
  onAvailableRef.current = onAvailableFormats;
  useEffect(() => {
    if (unavailable) onAvailableRef.current?.(unavailable.availableFormats);
  }, [unavailable]);
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
  const openGroupFromGraphRows = listOrMembers(
    createGroupRowOpener(groupController, graphRowsFrame, setGroupOrigin),
    { onOpenList, page: state.graphPage, frame: graphRowsFrame, text },
  );
  const openGroupFromTableRows = listOrMembers(
    createGroupRowOpener(groupController, tableRowsFrame, setGroupOrigin),
    { onOpenList, page: state.tablePage, frame: tableRowsFrame, text },
  );
  const intervals = {
    onOpenList,
    page: state.graphPage,
    frame: graphRowsFrame,
    text,
  };
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
          formatAction={formatAction}
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
          client={client}
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
          onActivate={(selection, shapeRef, kind) => {
            if (openInterval(intervals, selection, focusedElement())) return;
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
                  kind === "folder"
                    ? "folderExpandUnavailable"
                    : kind === "file"
                      ? "fileDetailUnavailable"
                      : "itemOpenUnavailable",
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
          onSelect={(selection, shapeRef) => {
            // A pointer press on an interval opens its list, as Enter does (README 52).
            if (openInterval(intervals, selection, focusedElement())) return;
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
          }}
          {...(openGroupFromGraphRows
            ? { onOpenGroup: openGroupFromGraphRows }
            : {})}
          returnTargetRef={visualizationTargetRef}
          rowsReturnTargetRef={graphRowsTargetRef}
          statusAction={statusAction}
          legend={legend}
          narrowing={narrowing}
          legendSide={legendSide}
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
      {shownGroups && (
        <GroupView
          controller={shownGroups}
          frame={groupViewFrame}
          selection={state.selection}
          origin={groupOrigin}
          returnTargetRef={groupReturnTargetRef}
          openRequest={openGroupRequest}
          text={text}
        />
      )}
      {/* Said once for assistive technology; the Toast shows the same words (F06 README 41). */}
      <div className="du-announcer">
        <LiveStatus>
          {announcement.message && (
            <Text key={announcement.count}>{announcement.message}</Text>
          )}
        </LiveStatus>
      </div>
      <Toast message={announcement.message} version={announcement.count} />
      <RowsRetry
        controller={controller}
        state={state}
        text={text}
        newer={!!graph && !currentTable}
      />
      <SeparateModelTable
        client={client}
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

type IntervalSource = {
  onOpenList: ((listing: IntervalListing) => void) | undefined;
  page: VisualizationSnapshot["graphPage"];
  frame: ModelResponse | null;
  text: VizText;
};

/** The element that has the focus, which takes it back when the list closes. */
function focusedElement() {
  const active = document.activeElement;
  return active instanceof HTMLElement || active instanceof SVGElement
    ? active
    : null;
}

/**
 * Opens the file list of a histogram interval (F06 README 12, 52) when the
 * selection is an interval with a list condition; true when it did.
 */
function openInterval(
  { onOpenList, page, frame, text }: IntervalSource,
  selection: VizSelection | { kind: "group"; groupRef: string },
  origin: HTMLElement | SVGElement | null,
) {
  if (!onOpenList || !frame || selection.kind !== "group") return false;
  const row = intervalRow(page, selection.groupRef);
  const listing = row && intervalListing(row, frame, text, origin);
  if (!listing) return false;
  onOpenList(listing);
  return true;
}

/** A table's group button: an interval opens its list, any other group its members. */
function listOrMembers(
  members: ReturnType<typeof createGroupRowOpener>,
  source: IntervalSource,
): ReturnType<typeof createGroupRowOpener> {
  if (!members && !source.onOpenList) return undefined;
  return (groupRef, size, origin, groupLabel, returnTarget) => {
    if (openInterval(source, { kind: "group", groupRef }, origin)) return;
    members?.(groupRef, size, origin, groupLabel, returnTarget);
  };
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

/** Combined items open only beside a chart; an unavailable answer has none (F06 README 41). */
function groupsBesideChart(
  groups: GroupController | undefined,
  unavailable: unknown,
) {
  return unavailable ? undefined : groups;
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
