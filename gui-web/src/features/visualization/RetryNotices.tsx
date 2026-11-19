import { useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import { Action, LiveStatus, Notice } from "../../components";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { VizRowsRequest } from "../../ipc";
import type { VizText } from "../../viz";

/**
 * Whether the arrival of the model now shown is worth one announcement: only
 * after the reader saw "working", or asked for a newer result. A first opening
 * and a chart that grows with a running scan are not announced.
 */
function useUpdateAnnouncement(
  state: VisualizationSnapshot,
  applying: RefObject<boolean>,
) {
  const sawWorking = useRef(false);
  const previous = useRef(state.phase);
  const [announce, setAnnounce] = useState(false);
  useLayoutEffect(() => {
    if (state.phase === previous.current) return;
    previous.current = state.phase;
    if (state.phase === "job") sawWorking.current = true;
    if (state.phase !== "model") return setAnnounce(false);
    setAnnounce(sawWorking.current || applying.current === true);
    sawWorking.current = false;
    applying.current = false;
  }, [state.phase, applying]);
  return announce;
}

export function RenderRetry({
  state,
  controller,
  text,
  applying,
}: {
  state: VisualizationSnapshot;
  controller: VisualizationController;
  text: VizText;
  applying: RefObject<boolean>;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const status = useRef<HTMLDivElement>(null);
  const busy = useRef(false);
  const [retained, setRetained] = useState(false);
  const [settled, setSettled] = useState(false);
  const announce = useUpdateAnnouncement(state, applying);
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
    // A retry the reader asked for says how it ended.
    applying.current = true;
    setRetained(true);
    void controller.retry().finally(() => setSettled(true));
  }
  return (
    <>
      {(state.phase === "error" || retained) && (
        <Notice tone="error">
          {/* With an earlier chart still shown, say that it is the one on screen. */}
          {state.graph ? (
            <>
              {text.message("renderFailedRetained")}{" "}
              {text.observedAt(state.graph.frame.observedAt)}
            </>
          ) : (
            text.message("renderFailed")
          )}{" "}
          <Action ref={button} aria-disabled={busy.current} onPress={retry}>
            {text.message(busy.current ? "retryingRender" : "retryRender")}
          </Action>
        </Notice>
      )}
      <div className="du-visually-hidden">
        <LiveStatus ref={status} focusTarget>
          {state.phase === "model"
            ? state.graph?.frame !== state.table
              ? text.message(state.graph ? "newerRowsStatus" : "rowsFromGraph")
              : announce
                ? text.message("graphUpdated")
                : ""
            : state.phase === "unavailable"
              ? // The notice announces itself; saying it here too would show it twice.
                ""
              : state.phase === "job"
                ? text.message("working")
                : state.phase === "cancelled"
                  ? text.message("cancelled")
                  : ""}
        </LiveStatus>
      </div>
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
        <div className="du-visually-hidden">
          <LiveStatus ref={status} focusTarget>
            {text.message(newer ? "newerRowsStatus" : "graphRowsStatus")}
          </LiveStatus>
        </div>
      )}
    </>
  );
}
