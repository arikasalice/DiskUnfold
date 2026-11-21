import { useId, useState } from "react";
import type { ReactNode } from "react";
import { ListBox, ListBoxItem, ToggleButton } from "react-aria-components";
import { Action } from "../controls";
import { ChartPatternDefs, patternId } from "./chart-patterns";
import type { ChartPattern } from "./chart-patterns";
import "./chart-legend.css";

/** One legend class as the screen shows it. */
export type LegendEntry = {
  key: string;
  label: string;
  count: string;
  total: string;
  tone: string;
  pattern: ChartPattern | null;
  patternAlways: boolean;
};

/** Legend items shown before "more" (F06 README 5). */
const SHOWN = 8;

/**
 * The legend of a chart (F06 README 5, 44): a header with the colour
 * criterion and the pattern switch, then one item per class with its swatch,
 * name, item count and total, in the engine's order. Choosing an item makes
 * its class stand out; choosing it again lets go. Arrow keys move in the
 * list and Space chooses. Past eight items the rest open with "more".
 */
export function ChartLegend({
  label,
  criterion,
  patternsLabel,
  patterns,
  patternsForced,
  onPatterns,
  entries,
  selectedKey,
  onSelect,
  moreLabel,
  lessLabel,
  footer,
}: {
  label: string;
  /** The colour criterion choice; it stays when no class is listed. */
  criterion: ReactNode;
  patternsLabel: string;
  patterns: boolean;
  /**
   * Why patterns are on whatever the switch says (forced colours, more
   * contrast): the switch then shows on and cannot be changed.
   */
  patternsForced?: string | undefined;
  onPatterns: (on: boolean) => void;
  entries: readonly LegendEntry[];
  selectedKey: string | null;
  onSelect: (key: string | null) => void;
  moreLabel: (hidden: number) => string;
  lessLabel: string;
  /** Lines under the list: the scope total, and why it can exceed the classes. */
  footer?: ReactNode;
}) {
  const prefix = useId();
  const forcedId = useId();
  const [open, setOpen] = useState(false);
  const shown = open ? entries : entries.slice(0, SHOWN);
  // Forced patterns are drawn in the chart, so the swatches show them too (#201).
  const drawn = patterns || patternsForced !== undefined;
  return (
    <section className="du-chart-legend" aria-label={label}>
      <svg
        className="du-chart-legend__defs"
        aria-hidden="true"
        focusable="false"
      >
        <defs>
          <ChartPatternDefs prefix={prefix} />
        </defs>
      </svg>
      <div className="du-chart-legend__head">
        {criterion}
        {entries.length > 0 && (
          // A disabled button shows no tooltip; the wrapper carries it.
          <span title={patternsForced}>
            <ToggleButton
              className="du-action du-chart-legend__patterns"
              isSelected={patterns || patternsForced !== undefined}
              isDisabled={patternsForced !== undefined}
              onChange={onPatterns}
              {...(patternsForced === undefined
                ? {}
                : { "aria-describedby": forcedId })}
            >
              {patternsLabel}
            </ToggleButton>
          </span>
        )}
        {patternsForced !== undefined && (
          <span id={forcedId} className="du-visually-hidden">
            {patternsForced}
          </span>
        )}
      </div>
      {entries.length > 0 && (
        <ListBox
          className="du-chart-legend__list"
          aria-label={label}
          selectionMode="single"
          selectedKeys={selectedKey === null ? [] : [selectedKey]}
          onSelectionChange={(keys) => {
            const [key] = keys === "all" ? [] : [...keys];
            onSelect(key === undefined ? null : String(key));
          }}
          items={shown}
        >
          {(entry) => (
            <ListBoxItem
              id={entry.key}
              className="du-chart-legend__item"
              textValue={`${entry.label}, ${entry.count}, ${entry.total}`}
            >
              {({ isSelected }) => (
                <>
                  {/* A chosen row has a check left of its swatch (README 46.4). */}
                  <span className="du-chart-legend__check" aria-hidden="true">
                    {isSelected ? "✓" : ""}
                  </span>
                  <Swatch entry={entry} patterns={drawn} prefix={prefix} />
                  <span className="du-chart-legend__name">{entry.label}</span>
                  <span className="du-chart-legend__number">{entry.count}</span>
                  <span className="du-chart-legend__number">{entry.total}</span>
                </>
              )}
            </ListBoxItem>
          )}
        </ListBox>
      )}
      {entries.length > SHOWN && (
        <Action onPress={() => setOpen((value) => !value)}>
          {open ? lessLabel : moreLabel(entries.length - SHOWN)}
        </Action>
      )}
      {footer}
    </section>
  );
}

/** The 12 px colour square of a class, with its pattern when it is drawn. */
function Swatch({
  entry,
  patterns,
  prefix,
}: {
  entry: LegendEntry;
  patterns: boolean;
  prefix: string;
}) {
  return (
    <svg
      className="du-chart-legend__swatch"
      data-tone={entry.tone}
      width={12}
      height={12}
      aria-hidden="true"
      focusable="false"
    >
      <rect width={12} height={12} rx={3} />
      {entry.pattern && (patterns || entry.patternAlways) && (
        <rect
          width={12}
          height={12}
          rx={3}
          className="du-chart-legend__pattern"
          fill={`url(#${patternId(prefix, entry.pattern)})`}
        />
      )}
    </svg>
  );
}
