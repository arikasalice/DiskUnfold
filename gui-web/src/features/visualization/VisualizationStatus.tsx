import { useRef } from "react";
import { Action, Notice, Progress, Text } from "../../components";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { VizText } from "../../viz";
import { RowName } from "../../viz";
import { RenderRetry } from "./RetryNotices";

export function VisualizationStatus({
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
  // Set when the reader asks for the newer result, so its arrival is announced.
  const applying = useRef(false);
  return (
    <>
      {state.phase === "loading" && (
        // Laid over the top of the view, so it moves nothing below it: the chart
        // keeps its place and its requested height (#163).
        <div className="du-viz-loading">
          <Progress label={text.message("loading")} />
        </div>
      )}
      {state.phase === "job" && <Notice>{text.message("working")}</Notice>}
      {state.phase === "cancelled" && (
        <Notice>{text.message("cancelled")}</Notice>
      )}
      <RenderRetry
        state={state}
        controller={controller}
        text={text}
        applying={applying}
      />
      {state.revisionCheck && (
        <Notice>
          {text.message(
            state.revisionCheck === "stale"
              ? "revisionStale"
              : "revisionUnverified",
          )}
        </Notice>
      )}
      {state.previousSelection && (
        <Notice>
          <Text>{text.message("selectionLost")}</Text>
          {state.previousSelectionEvidence?.name && (
            <Text>
              {text.message("previousName")}{" "}
              <RowName
                name={state.previousSelectionEvidence.name}
                item={state.previousSelection?.kind === "item"}
              />
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
                if (!state.offeredRevision) return;
                applying.current = true;
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
