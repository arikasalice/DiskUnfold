import "./slice-stack.css";
import { patternId } from "./chart-patterns";
import type { StackedBarSlice } from "./stacked-bar";

/** Surface between slices and the narrowest slice, in CSS px (F06 README 13). */
const GAP = 2;
const MIN = 2;

/** Where one part of a stack falls along its length. */
export type StackPart = { readonly start: number; readonly length: number };

/**
 * Lays `values` end to end over `length` with a gap between them, each as long
 * as its share and none shorter than the minimum, which the longer parts give up.
 * Null when the parts cannot all be drawn at the minimum.
 */
export function stackLayout(
  values: readonly number[],
  length: number,
): StackPart[] | null {
  const whole = values.reduce((sum, value) => sum + value, 0);
  const room = length - GAP * (values.length - 1);
  if (values.length === 0 || !(whole > 0) || room < MIN * values.length)
    return null;
  const lengths = values.map((value) => (room * value) / whole);
  // Raise short parts to the minimum and take it back from the rest, in proportion.
  for (let pass = 0; pass < values.length; pass += 1) {
    const short = lengths.filter((part) => part < MIN).length;
    if (short === 0) break;
    const free = lengths
      .filter((part) => part >= MIN)
      .reduce((sum, part) => sum + part, 0);
    const scale = (room - short * MIN) / free;
    for (let index = 0; index < lengths.length; index += 1)
      lengths[index] =
        lengths[index]! < MIN ? MIN : Math.max(MIN, lengths[index]! * scale);
  }
  let start = 0;
  return lengths.map((part) => {
    const placed = { start, length: part };
    start += part + GAP;
    return placed;
  });
}

type Box = { x: number; y: number; width: number; height: number };

/**
 * A folder's type slices across `box` in legend order, then what its value holds
 * beyond them in the neutral colour without a pattern (README 54). Nothing when
 * they do not fit.
 */
export function SliceStack({
  slices,
  total,
  box,
  patterns,
  prefix,
  chosen,
}: {
  slices: readonly StackedBarSlice[];
  total: number | undefined;
  box: Box;
  patterns: boolean;
  prefix: string;
  /** A class chosen in the legend: the other parts are drawn faded (README 4.7). */
  chosen?: string | undefined;
}) {
  const shown = slices.filter((slice) => slice.value > 0);
  const sum = shown.reduce((all, slice) => all + slice.value, 0);
  const rest = total !== undefined && total > sum ? total - sum : 0;
  const values = [
    ...shown.map((slice) => slice.value),
    ...(rest > 0 ? [rest] : []),
  ];
  const parts = stackLayout(values, box.width);
  if (!parts) return null;
  return (
    <g className="du-slice-stack" aria-hidden="true">
      {parts.map((part, index) => {
        const slice = shown[index];
        const at = {
          x: box.x + part.start,
          y: box.y,
          width: part.length,
          height: box.height,
        };
        const faded = chosen !== undefined && slice?.key !== chosen;
        if (!slice)
          return (
            <rect
              key="rest"
              className="du-slice-stack__rest"
              data-dimmed={faded ? "true" : undefined}
              {...at}
            />
          );
        const pattern =
          slice.pattern !== null && (patterns || slice.patternAlways)
            ? slice.pattern
            : null;
        return (
          <g
            key={slice.key}
            data-tone={slice.tone}
            data-key={slice.key}
            data-dimmed={faded ? "true" : undefined}
          >
            <rect className="du-slice-stack__slice" {...at} />
            {pattern && (
              <rect
                className="du-slice-stack__pattern"
                {...at}
                fill={`url(#${patternId(prefix, pattern)})`}
              />
            )}
          </g>
        );
      })}
    </g>
  );
}
