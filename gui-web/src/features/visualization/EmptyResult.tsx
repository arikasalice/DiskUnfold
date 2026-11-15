import { Notice, Text } from "../../components";
import type { ModelResponse, VizText } from "../../viz";

/**
 * Shown in place of a chart whose model has no shapes. It gives the scope
 * total and, when some items could not be confirmed, how many.
 */
export function EmptyResult({
  frame,
  text,
}: {
  frame: ModelResponse;
  text: VizText;
}) {
  const { totals, summary } = frame.model;
  const unknown = summary.unknownItems;
  const showUnknown = unknown.state !== "unknown" && unknown.value !== "0";
  return (
    <>
      <Notice>{text.message("emptyResultTitle")}</Notice>
      <Text>
        {text.message("emptyResultScopeTotal", {
          value: text.quantity(totals.scope.quantity),
        })}
      </Text>
      {showUnknown && (
        <Text>
          {text.message("emptyResultUnknownItems", {
            count: text.quantity(unknown),
          })}
        </Text>
      )}
    </>
  );
}
