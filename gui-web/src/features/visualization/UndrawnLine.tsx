import { useState } from "react";
import { Action, Text } from "../../components";
import type { VizRowsRequest } from "../../ipc";
import { vizMessage } from "../../viz";
import type { ModelResponse, ModelRowsPage, VizText } from "../../viz";

/** A row of the folder's table to open, scroll to and focus; a new token asks again. */
export type RowReveal = { row: number; token: number };

/**
 * The row the summary line's "View in list" reveals (F06 README 51.3, 48.4). It
 * asks for the page that holds the row when the shown page does not.
 */
export function useRowReveal(
  frame: ModelResponse,
  page: ModelRowsPage | undefined,
  onPage: ((request: VizRowsRequest) => void) | undefined,
) {
  const [reveal, setReveal] = useState<
    (RowReveal & { modelRef: string }) | null
  >(null);
  const current =
    reveal?.modelRef === frame.model.modelRef
      ? { row: reveal.row, token: reveal.token }
      : null;
  return {
    reveal: current,
    open: (row: number) => {
      setReveal({
        row,
        token: (reveal?.token ?? 0) + 1,
        modelRef: frame.model.modelRef,
      });
      if (!page || !onPage) return;
      const { request, response } = page;
      const shown =
        row >= request.start && row < request.start + response.rows.length;
      if (!shown && request.count > 0)
        onPage({
          ...request,
          start: Math.floor(row / request.count) * request.count,
        });
    },
  };
}

/**
 * "Items not drawn: 3 · at least 1.2 GiB · View in list" (F06 README 51.3). The
 * line stands only when the engine names the first such row; the size part only
 * when the size is known. The tooltip says why the areas leave them out (51.4).
 */
export function UndrawnLine({
  frame,
  text,
  onOpen,
}: {
  frame: ModelResponse;
  text: VizText;
  onOpen: (row: number) => void;
}) {
  const { unknownItems, unknownSize, firstUnknownRow } = frame.model.summary;
  if (firstUnknownRow === undefined || unknownItems.state === "unknown")
    return null;
  const count = vizMessage(text, "undrawnCount", {
    count: unknownItems.value,
  });
  const summary =
    unknownSize && unknownSize.state !== "unknown"
      ? `${count} · ${vizMessage(text, "undrawnAtLeast", {
          size: text.quantity({ ...unknownSize, state: "exact" }),
        })}`
      : count;
  return (
    <div className="du-undrawn-line" title={text.message("undrawnTooltip")}>
      <Text>{summary}</Text>
      <Action
        variant="quiet"
        aria-label={vizMessage(text, "undrawnShowInListName", { summary })}
        onPress={() => onOpen(firstUnknownRow)}
      >
        {text.message("undrawnShowInList")}
      </Action>
    </div>
  );
}
