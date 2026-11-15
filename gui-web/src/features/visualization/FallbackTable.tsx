import { useEffect } from "react";
import { Progress } from "../../components";
import type { VizText } from "../../viz";
import type {
  VisualizationController,
  VisualizationSnapshot,
} from "./controller";
import type { FallbackReference } from "./fallback-table";
import { ModelRows } from "./ModelRows";

const FIRST_PAGE_SIZE = 100;

/**
 * The rows of the fallback model under an unavailable notice. The first page is
 * read when the notice appears; paging and the retry of a failed read go
 * through the controller, which drops any page that arrives for an answer that
 * is no longer current.
 */
export function FallbackTable({
  controller,
  state,
  text,
  reference,
}: {
  controller: VisualizationController;
  state: VisualizationSnapshot;
  text: VizText;
  reference: FallbackReference;
}) {
  const requested = Boolean(
    state.fallbackPage || state.pendingRows || state.failedRows,
  );
  useEffect(() => {
    if (!requested)
      void controller.rows({
        modelRef: reference.modelRef,
        resultRevision: reference.resultRevision,
        start: 0,
        count: FIRST_PAGE_SIZE,
      });
  }, [controller, reference.modelRef, reference.resultRevision, requested]);
  return (
    <>
      {state.pendingRows && <Progress label={text.message("loadingNewer")} />}
      {state.fallbackPage && (
        <ModelRows
          expanded
          page={state.fallbackPage}
          text={text}
          onPage={(request) => {
            void controller.rows(request);
          }}
        />
      )}
    </>
  );
}
