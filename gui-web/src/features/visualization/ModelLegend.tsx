import { useMemo, useState, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import { Action, ChartLegend, Text } from "../../components";
import type { GeometryRendererProps, LegendEntry } from "../../components";
import { hasClosedFolders, vizMessage } from "../../viz";
import type { ModelResponse, VizText } from "../../viz";

/** The colour criterion choice and the pattern switch, kept by the screen. */
export type LegendControls = {
  criterion: ReactNode;
  patterns: boolean;
  onPatterns: (on: boolean) => void;
};

const AUTO_PATTERNS = "(forced-colors: active), (prefers-contrast: more)";

/**
 * "Automatic" patterns (F06 README 4.1): on in forced colours or when the
 * system asks for more contrast, whatever the switch says.
 */
export function useAutoPatterns() {
  return useSyncExternalStore(
    (changed) => {
      const query = window.matchMedia?.(AUTO_PATTERNS);
      query?.addEventListener("change", changed);
      return () => query?.removeEventListener("change", changed);
    },
    () => window.matchMedia?.(AUTO_PATTERNS).matches ?? false,
    () => false,
  );
}

/**
 * The class chosen in the legend and the shapes it leaves faded (README 4.7).
 * A choice the model no longer lists lets go by itself.
 */
export function useClassChoice(
  entries: readonly LegendEntry[],
  shapes: GeometryRendererProps["shapes"],
) {
  const [chosen, setChosen] = useState<string | null>(null);
  const selected =
    chosen !== null && entries.some((entry) => entry.key === chosen)
      ? chosen
      : null;
  const drawn = useMemo(
    () =>
      selected === null
        ? shapes
        : shapes.map((shape) =>
            shape.legendClass !== undefined && shape.legendClass !== selected
              ? { ...shape, dimmed: true }
              : shape,
          ),
    [selected, shapes],
  );
  return { selected, choose: setChosen, shapes: drawn };
}

/**
 * The class chosen in the legend as a chip on the status line, "Type: Videos ×"
 * (README 5): it stays in view when the legend is scrolled away, and pressing
 * it lets go of the class. Nothing when no class is chosen.
 */
export function ClassChip({
  entries,
  selected,
  onClear,
  text,
}: {
  entries: readonly LegendEntry[];
  selected: string | null;
  onClear: () => void;
  text: VizText;
}) {
  const entry = entries.find((candidate) => candidate.key === selected);
  if (!entry) return null;
  const label = vizMessage(text, "legendChip", {
    criterion: text.message("colorType"),
    name: entry.label,
  });
  return (
    <Action
      onPress={onClear}
      aria-label={vizMessage(text, "legendChipClear", { name: label })}
    >
      {`${label} ×`}
    </Action>
  );
}

/** The legend of a model, under its colour criterion choice (F06 README 5, 44). */
export function ModelLegend({
  frame,
  text,
  controls,
  entries,
  selected,
  onSelect,
  forced,
}: {
  frame: ModelResponse;
  text: VizText;
  /** Absent where the format has no legend: nothing is shown. */
  controls: LegendControls | undefined;
  entries: readonly LegendEntry[];
  selected: string | null;
  onSelect: (key: string | null) => void;
  /** Patterns are on by the system's request (forced colours, more contrast). */
  forced: boolean;
}) {
  if (!controls) return null;
  const footer = entries.length > 0 && (
    <div className="du-chart-legend__foot">
      <Text>
        {vizMessage(text, "scopeTotalLine", {
          value: text.quantity(frame.model.totals.scope.quantity),
        })}
      </Text>
      {hasClosedFolders(frame.model) && (
        <Text kind="muted">{text.message("legendFolderNote")}</Text>
      )}
    </div>
  );
  return (
    <ChartLegend
      label={text.message("legendTitle")}
      criterion={controls.criterion}
      patternsLabel={text.message("legendPatterns")}
      patterns={controls.patterns}
      patternsForced={forced ? text.message("legendPatternsForced") : undefined}
      onPatterns={controls.onPatterns}
      entries={entries}
      selectedKey={selected}
      onSelect={onSelect}
      moreLabel={(hidden) =>
        vizMessage(text, "legendMore", { count: String(hidden) })
      }
      lessLabel={text.message("legendLess")}
      footer={footer}
    />
  );
}
