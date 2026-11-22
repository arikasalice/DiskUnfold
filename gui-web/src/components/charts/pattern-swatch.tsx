import { ChartPatternDefs, patternId } from "./chart-patterns";
import type { ChartPattern } from "./chart-patterns";

const SIZE = 10;

/**
 * A small colour square with the pattern of its class, as the chart legend draws its
 * swatches. A list beside a chart uses it so that, in forced colours, classes differ
 * by pattern (F06 README 44.4). The caller styles the tones through `className`.
 */
export function PatternSwatch({
  className,
  tone,
  pattern,
  drawPattern,
  prefix,
}: {
  className: string;
  tone: string;
  pattern: ChartPattern | null;
  drawPattern: boolean;
  prefix: string;
}) {
  return (
    <svg
      className={className}
      data-tone={tone}
      width={SIZE}
      height={SIZE}
      aria-hidden="true"
      focusable="false"
    >
      <rect width={SIZE} height={SIZE} rx={2} />
      {pattern && drawPattern && (
        <rect
          width={SIZE}
          height={SIZE}
          rx={2}
          fill={`url(#${patternId(prefix, pattern)})`}
        />
      )}
    </svg>
  );
}

/** The pattern definitions the swatches of one list refer to, under ids from `prefix`. */
export function PatternSwatchDefs({
  className,
  prefix,
}: {
  className: string;
  prefix: string;
}) {
  return (
    <svg className={className} aria-hidden="true" focusable="false">
      <defs>
        <ChartPatternDefs prefix={prefix} />
      </defs>
    </svg>
  );
}
