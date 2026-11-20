import type { VizText } from "../../viz";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import { fallbackReference } from "./fallback-table";
import { FallbackTable } from "./FallbackTable";
import { UnavailablePanel } from "./UnavailablePanel";
import type { BackAction } from "./UnavailablePanel";

/** The unavailable notice and, when the answer names a fallback model, its rows. */
export function UnavailableView({
  reasonKey,
  completeness,
  sizeBasis,
  controller,
  state,
  text,
  backAction,
}: {
  reasonKey: string;
  completeness?: string | undefined;
  sizeBasis?: string | undefined;
  controller: VisualizationController;
  state: VisualizationSnapshot;
  text: VizText;
  backAction?: BackAction | undefined;
}) {
  const reference = fallbackReference(state);
  return (
    <>
      <UnavailablePanel
        reasonKey={reasonKey}
        completeness={completeness}
        sizeBasis={sizeBasis}
        text={text}
        backAction={backAction}
      />
      {reference && (
        <FallbackTable
          controller={controller}
          state={state}
          text={text}
          reference={reference}
        />
      )}
    </>
  );
}
