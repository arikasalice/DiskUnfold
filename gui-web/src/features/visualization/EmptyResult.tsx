import { Notice, Text } from "../../components";
import type { ModelResponse, VizText } from "../../viz";

/**
 * Shown in place of a chart whose model has no shapes. A folder with no items
 * says it is empty; a folder with items that all draw nothing says there is
 * nothing to draw. Before the first rows are read the number of items is not
 * known, so neither sentence is said. When some items could not be confirmed,
 * their number follows.
 */
export function EmptyResult({
  frame,
  text,
  rowsTotal,
}: {
  frame: ModelResponse;
  text: VizText;
  /** The number of items of the folder, or null while the rows are not read. */
  rowsTotal: number | null;
}) {
  const unknown = frame.model.summary.unknownItems;
  const showUnknown = unknown.state !== "unknown" && unknown.value !== "0";
  if (rowsTotal === null) return null;
  return (
    <>
      <Notice>
        {text.message(
          rowsTotal === 0 ? "emptyFolderTitle" : "emptyResultTitle",
        )}
      </Notice>
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
