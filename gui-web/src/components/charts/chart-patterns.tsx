/**
 * The category patterns of the chart palette (F06 README 4.1): one per palette
 * place, and the two neutral ones. Lines are 1 CSS px, 6 px apart (the dense
 * hatch 4), in the fill text colour at 35% opacity.
 */
export const CHART_PATTERNS = [
  "hatch-up",
  "dots",
  "lines-h",
  "hatch-down",
  "grid",
  "lines-v",
  "rings",
  "dense-hatch",
  "sparse-dots",
] as const;
export type ChartPattern = (typeof CHART_PATTERNS)[number];

/** The pattern of a palette place; place 1 is plain. */
export const PLACE_PATTERNS: readonly (ChartPattern | null)[] = [
  null,
  "hatch-up",
  "dots",
  "lines-h",
  "hatch-down",
  "grid",
  "lines-v",
  "rings",
];

export const patternId = (prefix: string, pattern: ChartPattern) =>
  `${prefix}-${pattern}`;

function tile(pattern: ChartPattern) {
  switch (pattern) {
    case "hatch-up":
    case "hatch-down":
    case "dense-hatch":
      return (
        <line x1={0} y1={0} x2={0} y2={pattern === "dense-hatch" ? 4 : 6} />
      );
    case "lines-h":
      return <line x1={0} y1={0.5} x2={6} y2={0.5} />;
    case "lines-v":
      return <line x1={0.5} y1={0} x2={0.5} y2={6} />;
    case "grid":
      return <path d="M0 0.5H6M0.5 0V6" fill="none" />;
    case "dots":
      return <circle cx={3} cy={3} r={1} className="du-pattern-mark--fill" />;
    case "rings":
      return <circle cx={3} cy={3} r={1.75} fill="none" />;
    case "sparse-dots":
      return <circle cx={5} cy={5} r={1} className="du-pattern-mark--fill" />;
  }
}

const SIZE: Record<ChartPattern, number> = {
  "hatch-up": 6,
  dots: 6,
  "lines-h": 6,
  "hatch-down": 6,
  grid: 6,
  "lines-v": 6,
  rings: 6,
  "dense-hatch": 4,
  "sparse-dots": 10,
};

const ROTATION: Partial<Record<ChartPattern, number>> = {
  "hatch-up": 45,
  "hatch-down": -45,
  "dense-hatch": -45,
};

/** The pattern definitions of one drawing, under ids made from `prefix`. */
export function ChartPatternDefs({ prefix }: { prefix: string }) {
  return (
    <>
      {CHART_PATTERNS.map((pattern) => (
        <pattern
          key={pattern}
          id={patternId(prefix, pattern)}
          className={`du-chart-pattern du-chart-pattern--${pattern}`}
          width={SIZE[pattern]}
          height={SIZE[pattern]}
          patternUnits="userSpaceOnUse"
          {...(ROTATION[pattern] === undefined
            ? {}
            : { patternTransform: `rotate(${ROTATION[pattern]})` })}
        >
          {tile(pattern)}
        </pattern>
      ))}
    </>
  );
}
