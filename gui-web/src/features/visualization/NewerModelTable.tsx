import { useEffect, useId, useRef } from "react";
import { Action, Heading, Progress, Text } from "../../components";
import { ModelRows } from "./VisualizationModelPresenter";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { VizRow } from "../../ipc";
import type { VizText } from "../../viz";

/**
 * Rows read for the first table page; a model with more shapes reads one per
 * shape. Without a chart the table is the only way to the values, so its first
 * page is read even when the owner did not ask for automatic rows.
 */
const FIRST_ROWS = 100;

export function useFirstRows(
  controller: VisualizationController,
  state: VisualizationSnapshot,
  autoLoadRows: boolean,
) {
  const table = state.table;
  const load = autoLoadRows || !state.graph;
  useEffect(() => {
    if (
      !load ||
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
      count: Math.max(FIRST_ROWS, table.model.shapes.length),
    });
  }, [
    load,
    controller,
    state.phase,
    state.tablePage,
    state.pendingRows,
    state.failedRows,
    table,
  ]);
}

export function SeparateModelTable({
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
  // Without a chart this is the folder's only table; beside a pinned chart it is the newer result.
  const alone = !graph;
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
            {text.message(alone ? "rowsFromGraph" : "newerRegion")}
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
            <Progress
              label={text.message(alone ? "loadingGraphRows" : "loadingNewer")}
            />
          )}
          {/* Beside a pinned chart, moving to the newer result is the reader's choice. */}
          {!alone &&
            !state.tablePage &&
            !state.pendingRows &&
            !state.failedRows && (
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
              title={text.message(alone ? "rowsFromGraph" : "newerTitle")}
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
