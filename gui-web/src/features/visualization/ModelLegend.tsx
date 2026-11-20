import { useMemo, useState, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import { Action, ChartLegend, Text } from "../../components";
import type { GeometryRendererProps, LegendEntry } from "../../components";
import { hasClosedFolders, TYPE_CRITERION, vizMessage } from "../../viz";
import type { ModelResponse, VizText } from "../../viz";
import type { QueryCondition } from "../../ipc";

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

/** The type the legend narrows to (F06 README 46), kept by the screen. */
export type TypeNarrowing = {
  key: string | null;
  label: string | null;
  /** Chosen, but the format shown does not narrow (README 46.5). */
  inactive: boolean;
  /** What the last change said once to a screen reader (README 46.6). */
  announced: { type: string } | "all" | null;
  choose: (
    choice: { key: string; label: string; condition: QueryCondition } | null,
  ) => void;
};

/** Whether this frame is drawn narrowed to the chosen type. */
export function narrowedFrame(
  narrowing: TypeNarrowing | undefined,
  frame: ModelResponse,
) {
  return (
    !!narrowing?.key &&
    !narrowing.inactive &&
    frame.model.representation.colorCriterion === TYPE_CRITERION
  );
}

/**
 * The chosen type on the status line (README 46.2): the chip "Type: Videos ×",
 * then the narrowed total against the scope total and what the filter left
 * out; inactive with a note where the format does not narrow.
 */
export function NarrowingStatus({
  narrowing,
  frame,
  text,
}: {
  narrowing: TypeNarrowing | undefined;
  frame: ModelResponse;
  text: VizText;
}) {
  if (!narrowing) return null;
  const announcement =
    narrowing.announced === null
      ? ""
      : narrowing.announced === "all"
        ? text.message("filterAnnounceAll")
        : vizMessage(text, "filterAnnounceType", narrowing.announced);
  const live = (
    <div className="du-visually-hidden" role="status" aria-live="polite">
      {announcement}
    </div>
  );
  if (!narrowing.key || !narrowing.label) return live;
  const label = vizMessage(text, "legendChip", {
    criterion: text.message("colorType"),
    name: narrowing.label,
  });
  const narrowed = narrowedFrame(narrowing, frame);
  return (
    <>
      {live}
      <div
        className="du-narrowing-chip"
        data-inactive={narrowing.inactive || undefined}
      >
        <Action
          onPress={() => narrowing.choose(null)}
          aria-label={text.message("filterClearType")}
        >
          {`${label} ×`}
        </Action>
        {narrowing.inactive && (
          <Text kind="muted">{text.message("filterNotApplicable")}</Text>
        )}
      </div>
      {narrowed && (
        <NarrowedTotals
          narrowing={narrowing}
          frame={frame}
          text={text}
          label={narrowing.label}
        />
      )}
    </>
  );
}

function NarrowedTotals({
  narrowing,
  frame,
  text,
  label,
}: {
  narrowing: TypeNarrowing;
  frame: ModelResponse;
  text: VizText;
  label: string;
}) {
  const { totals, legend } = frame.model;
  const entry = legend.find((candidate) => candidate.key === narrowing.key);
  const removed = filteredOutText(frame, text);
  return (
    <>
      {entry && (
        <Text>
          {vizMessage(text, "filterTotalLine", {
            type: label,
            size: text.quantity(entry.total.quantity),
            scope: text.quantity(totals.scope.quantity),
          })}
        </Text>
      )}
      {removed && <Text kind="muted">{removed}</Text>}
    </>
  );
}

/** "Filtered out" by the value state (README 46.7); nothing when nothing was left out. */
function filteredOutText(frame: ModelResponse, text: VizText) {
  const quantity = frame.model.totals.filteredOut.quantity;
  if (quantity.state === "exact")
    return quantity.value === "0"
      ? null
      : vizMessage(text, "filterFilteredOut", {
          size: text.quantity(quantity),
        });
  if (quantity.state === "lower_bound")
    return vizMessage(text, "filterFilteredOutAtLeast", {
      size: text.quantity({ ...quantity, state: "exact" }),
    });
  return text.message(
    frame.completeness === "partial"
      ? "filterFilteredOutCalculating"
      : "filterFilteredOutUnavailable",
  );
}

/** In the chart's place when the folder holds no file of the chosen type (README 46.1). */
export function TypeAbsent({
  narrowing,
  text,
}: {
  narrowing: TypeNarrowing;
  text: VizText;
}) {
  return (
    <div className="du-type-absent">
      <Text>
        {vizMessage(text, "filterTypeAbsent", {
          type: narrowing.label ?? "",
        })}
      </Text>
      <Action onPress={() => narrowing.choose(null)}>
        {text.message("filterClearType")}
      </Action>
    </div>
  );
}
