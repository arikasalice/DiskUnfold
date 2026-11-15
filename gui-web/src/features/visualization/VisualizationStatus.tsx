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
