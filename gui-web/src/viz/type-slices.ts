import type { StackedBarSlice } from "../components";
import type { SizeValue, VizQuantity, VizTypeSlice } from "../ipc";
import { legendLook } from "./legend-style";
import type { VisualizationMessageKey } from "./messages";

/** How many types a card or an accessible name lists before the rest (F06 README 54.5). */
export const NAMED_TYPES = 5;

type SliceText = {
  readonly message: (
    key: VisualizationMessageKey,
    values?: Readonly<Record<string, string | bigint>>,
  ) => string;
  readonly quantity: (value: VizQuantity) => string;
  /** The name of a legend class in the active language. */
  readonly className: (key: string) => string;
};

/** What a folder's type slices show (F06 README 54; #298). */
export type FolderTypes = {
  /** The bar: slices in legend order, so every bar puts a type in the same place. */
  readonly bar: readonly StackedBarSlice[];
  /** The folder's bytes, when known: what they hold beyond the slices is drawn last. */
  readonly total: number | undefined;
  /** Card and accessible-name lines: the largest types, the rest, the part not split. */
  readonly said: readonly string[];
  /** The table's class cell: "Documents + 3 more", one type's name, or "—". */
  readonly cell: string;
};

const bytesOf = (value: VizQuantity) =>
  value.state === "unknown" ? 0 : Number(value.value);

function bytes(value: number, lowerBound: boolean): VizQuantity {
  return {
    unit: "bytes",
    state: lowerBound ? "lower_bound" : "exact",
    value: String(value),
  };
}

/**
 * What `slices` of a folder of `size` show, or null for a row or shape without
 * them. `order` is the model's legend order.
 */
export function folderTypes(
  slices: readonly VizTypeSlice[] | undefined,
  size: SizeValue,
  order: readonly string[],
  text: SliceText,
): FolderTypes | null {
  if (slices === undefined) return null;
  const fill = (
    template: VisualizationMessageKey,
    values: Record<string, string>,
  ) =>
    text
      .message(template, values)
      .replace(/\{(\w+)\}/gu, (whole, name: string) => values[name] ?? whole);
  const named = slices
    .slice(0, NAMED_TYPES)
    .map(
      (slice) =>
        `${text.className(slice.classKey)} ${text.quantity(slice.total)}`,
    );
  const rest = slices.slice(NAMED_TYPES);
  const restLines =
    rest.length === 0
      ? []
      : [
          fill("typeSlicesRest", {
            count: String(rest.length),
            size: text.quantity(
              bytes(
                rest.reduce((sum, slice) => sum + bytesOf(slice.total), 0),
                rest.some((slice) => slice.total.state === "lower_bound"),
              ),
            ),
          }),
        ];
  const sum = slices.reduce((all, slice) => all + bytesOf(slice.total), 0);
  const folder = size.state === "unknown" ? undefined : Number(size.bytes);
  const partial =
    size.state !== "exact" ||
    slices.some((slice) => slice.total.state !== "exact");
  // README 54: when the value or a slice is a lower bound, what is beyond the
  // slices is never read as an exact remainder.
  const unsplit =
    partial && folder !== undefined && folder > sum
      ? [
          fill("typeSlicesUnsplit", {
            size: text.quantity(bytes(folder - sum, false)),
          }),
        ]
      : [];
  const first = slices[0];
  const cell =
    first === undefined
      ? "—"
      : slices.length === 1
        ? text.className(first.classKey)
        : fill("typeSlicesMore", {
            name: text.className(first.classKey),
            count: String(slices.length - 1),
          });
  const place = (key: string) => {
    const index = order.indexOf(key);
    return index < 0 ? order.length : index;
  };
  const bar = [...slices]
    .sort((a, b) => place(a.classKey) - place(b.classKey))
    .map((slice) => ({
      key: slice.classKey,
      ...legendLook(slice.classKey, order),
      value: bytesOf(slice.total),
    }));
  return {
    bar,
    total: folder,
    said: [...named, ...restLines, ...unsplit],
    cell,
  };
}
