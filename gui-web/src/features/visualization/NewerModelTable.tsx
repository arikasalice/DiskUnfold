import { useEffect, useId, useRef } from "react";
import { Action, Heading, Progress, Text } from "../../components";
import { ModelRows } from "./VisualizationModelPresenter";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { VizRow } from "../../ipc";
import type { VizText } from "../../viz";

export function useFirstRows(
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
