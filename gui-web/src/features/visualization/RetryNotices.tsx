import { useLayoutEffect, useRef, useState } from "react";
import { Action, LiveStatus, Notice } from "../../components";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { VizRowsRequest } from "../../ipc";
import type { VizText } from "../../viz";

export function RenderRetry({
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
            ? // The notice announces itself; saying it here too would show it twice.
              ""
            : state.phase === "job"
              ? text.message("working")
              : state.phase === "cancelled"
                ? text.message("cancelled")
                : ""}
      </LiveStatus>
    </>
  );
}

export function RowsRetry({
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
