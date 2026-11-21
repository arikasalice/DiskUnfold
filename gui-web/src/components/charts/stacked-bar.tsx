import { useId } from "react";
import { ChartPatternDefs, patternId } from "./chart-patterns";
import type { ChartPattern } from "./chart-patterns";
import "./stacked-bar.css";

/** One slice: the legend look of its class and its value, in one unit for all. */
export type StackedBarSlice = {
  readonly key: string;
  readonly tone: string;
  readonly pattern: ChartPattern | null;
  readonly patternAlways: boolean;
  readonly value: number;
};

export type StackedBarProps = {
  /** Drawn in this order; a slice without a value is left out. */
  readonly slices: readonly StackedBarSlice[];
  /**
   * The whole the slices belong to. What it holds beyond them is drawn last in
   * the neutral colour without a pattern, never as a slice (F06 README 54).
   */
  readonly total?: number;
  /** 8 in a table row, 12 in a summary card, 20 with a name inside (README 13). */
  readonly height?: 8 | 12 | 20;
  /** Patterns on every slice: the pattern switch, or forced colours. */
  readonly patterns?: boolean;
};

/**
 * A one-line stacked bar (F06 README 13): slices as long as their values, 2 px of
 * surface between them, none narrower than 2 px, ends rounded by 4 px. It is
 * decorative; the screen that shows it says the same in text.
 */
export function StackedBar({
  slices,
  total,
  height = 8,
  patterns = false,
}: StackedBarProps) {
  const prefix = useId();
  const shown = slices.filter((slice) => slice.value > 0);
  const sum = shown.reduce((all, slice) => all + slice.value, 0);
  const rest = total !== undefined && total > sum ? total - sum : 0;
  const whole = sum + rest;
  if (!(whole > 0)) return null;
  const patterned = (slice: StackedBarSlice) =>
    slice.pattern !== null && (patterns || slice.patternAlways)
      ? slice.pattern
      : null;
  return (
    <div className="du-stacked-bar" data-height={height} aria-hidden="true">
      {shown.some((slice) => patterned(slice) !== null) && (
        <svg className="du-stacked-bar__defs" width={0} height={0}>
          <defs>
            <ChartPatternDefs prefix={prefix} />
          </defs>
        </svg>
      )}
      {shown.map((slice) => {
        const pattern = patterned(slice);
        return (
          <svg
            key={slice.key}
            className="du-stacked-bar__slice"
            data-key={slice.key}
            data-tone={slice.tone}
            style={{ flexGrow: slice.value / whole }}
            preserveAspectRatio="none"
          >
            <rect width="100%" height="100%" />
            {pattern !== null && (
              <rect
                width="100%"
                height="100%"
                className="du-stacked-bar__pattern"
                fill={`url(#${patternId(prefix, pattern)})`}
              />
            )}
          </svg>
        );
      })}
      {rest > 0 && (
        <div
          className="du-stacked-bar__rest"
          style={{ flexGrow: rest / whole }}
        />
      )}
    </div>
  );
}
