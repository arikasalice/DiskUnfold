import { useMemo, useState, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import { ChartLegend, Text } from "../../components";
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

/** The legend of a model, under its colour criterion choice (F06 README 5, 44). */
export function ModelLegend({
  frame,
  text,
  controls,
  entries,
  selected,
  onSelect,
}: {
  frame: ModelResponse;
  text: VizText;
  /** Absent where the format has no legend: nothing is shown. */
  controls: LegendControls | undefined;
  entries: readonly LegendEntry[];
  selected: string | null;
  onSelect: (key: string | null) => void;
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
