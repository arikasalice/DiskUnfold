import type { QueryCondition, VizQuantity, VizRow } from "../../ipc";
import { vizMessage } from "../../viz";
import type { ModelResponse, ModelRowsPage, VizText } from "../../viz";
import type { TypeListSummary } from "./ModelLegend";

/** A histogram interval's file list, open in the chart's place (F06 README 12, 52). */
export type IntervalListing = {
  condition: QueryCondition;
  /** "Size 1 MB to under 10 MB", from the condition's exact bounds. */
  label: string;
  summary: TypeListSummary;
  /** The bar or button that opened it; it takes the focus back. */
  origin: HTMLElement | SVGElement | null;
  /** The model it came from; the list is shown only for that model. */
  modelRef: string;
};

/** The interval row of a page by its group reference, when it can open a list. */
export function intervalRow(
  page: ModelRowsPage | null | undefined,
  groupRef: string,
): VizRow | null {
  const row = page?.response.rows.find(
    (candidate) => candidate.groupRef === groupRef,
  );
  const count = row?.memberCount;
  if (!row?.condition || !count) return null;
  return count.state === "exact" && count.value === "0" ? null : row;
}

/** The listing an interval row opens, or none when its bounds cannot be read. */
export function intervalListing(
  row: VizRow,
  frame: ModelResponse,
  text: VizText,
  origin: HTMLElement | SVGElement | null,
): IntervalListing | null {
  const label = row.condition ? intervalLabel(row.condition, text) : null;
  if (!row.condition || !label || !row.memberCount) return null;
  return {
    condition: row.condition,
    label,
    summary: {
      count: row.memberCount,
      size: bytesQuantity(row.size),
      sizeBasis: frame.model.representation.sizeBasis,
    },
    origin,
    modelRef: frame.model.modelRef,
  };
}

/** "Size 1 MB to under 10 MB", or "Size 1 TB or more" for the last interval. */
export function intervalLabel(
  condition: QueryCondition,
  text: VizText,
): string | null {
  if (condition.kind !== "all") return null;
  let lower: string | null = null;
  let upper: string | null = null;
  for (const child of condition.children) {
    if (child.kind !== "predicate" || child.predicate.kind !== "size") continue;
    // Bounds are whole units, named as the bars name them: "1 MiB", not "1.0 MiB".
    const bytes = text
      .quantity({ unit: "bytes", state: "exact", value: child.predicate.bytes })
      .replace(/(\d)[.,]0+(?=\D|$)/u, "$1");
    if (child.predicate.op === "at_least") lower = bytes;
    else if (child.predicate.op === "less_than") upper = bytes;
  }
  if (lower === null) return null;
  return upper === null
    ? vizMessage(text, "intervalFrom", { lower })
    : vizMessage(text, "intervalRange", { lower, upper });
}

function bytesQuantity(size: VizRow["size"]): VizQuantity {
  return size.state === "unknown"
    ? { unit: "bytes", state: "unknown", reasonKey: "space.data_insufficient" }
    : { unit: "bytes", state: size.state, value: size.bytes };
}
