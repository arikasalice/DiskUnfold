import { useState } from "react";
import { SegmentedChoice, Text } from "../components";
import type { QueryCondition, VizCatalog, VizRenderRequest } from "../ipc";
import { TYPE_CRITERION } from "../viz";
import type { VisualizationViewRequest, VizText } from "../viz";
import type { TypeNarrowing } from "../features/visualization/ModelLegend";

const NONE = "none";
/** The legend column and the gap before it, in CSS px (README 2). */
const LEGEND_SPACE = 240 + 16;
/** From this chart area width the legend stands beside the chart (README 2). */
const LEGEND_BESIDE = 720;
/** Formats whose shapes a type condition narrows (#301). */
const NARROWED_FORMATS: ReadonlySet<string> = new Set(["treemap", "bar"]);

type Choice = { key: string; label: string; condition: QueryCondition };

/**
 * The type the legend narrows to (F06 README 46). It is kept while the screen
 * is used, across folders and formats: a format that does not narrow keeps it
 * and shows it inactive, and the treemap applies it again. It is not sent
 * without the type criterion, and comes back with it.
 */
function useTypeNarrowing(format: string, criterion: string) {
  const [choice, setChoice] = useState<Choice | null>(null);
  const [announced, setAnnounced] = useState<TypeNarrowing["announced"]>(null);
  const [seen, setSeen] = useState<Seen>({ scanning: false, grouped: false });
  const shown = criterion === TYPE_CRITERION ? choice : null;
  const reason = shown === null ? null : refusal(format, seen);
  const applied = shown !== null && reason === null;
  const condition: Pick<VizRenderRequest, "condition"> = applied
    ? { condition: { kind: "normalized", condition: shown.condition } }
    : {};
  const narrowing: TypeNarrowing = {
    key: shown?.key ?? null,
    label: shown?.label ?? null,
    inactive: reason !== null,
    reason,
    announced,
    choose: (next) => {
      setChoice(next);
      setAnnounced(next ? { type: next.label } : "all");
    },
    report: (facts) =>
      setSeen((current) => {
        const next = {
          scanning: facts.scanning,
          grouped: facts.format === "bar" ? facts.grouped : current.grouped,
        };
        return next.scanning === current.scanning &&
          next.grouped === current.grouped
          ? current
          : next;
      }),
  };
  return { condition, narrowing };
}

/** What the screen last saw: a scan still running, and a bar chart with an Other group. */
type Seen = { scanning: boolean; grouped: boolean };

/**
 * Why the chosen type is not sent (README 46.5, 46.12). The bar chart narrows
 * only a finished result whose children fit without an Other group; narrowed
 * children are a subset, so an Other group in the bar chart shown means the
 * narrowed one needs one too. The engine refuses those cases the same way.
 */
function refusal(format: string, seen: Seen): TypeNarrowing["reason"] {
  if (!NARROWED_FORMATS.has(format)) return "format";
  if (format !== "bar") return null;
  if (seen.scanning) return "scanning";
  return seen.grouped ? "too_many" : null;
}

/**
 * The colour criterion and the legend of the space view (F06 README 44.1).
 *
 * The treemap and the bar chart start coloured by type when the catalog serves
 * it; the choice and the pattern switch are kept while the screen is used and
 * every folder shares them, and a restart starts on type again. A histogram
 * has one colour and no legend. The legend stands beside a chart area of
 * 720 px or more, which then asks for a chart narrower by the legend column.
 */
export function useLegendChoice(
  format: string,
  catalog: VizCatalog | undefined,
  owner:
    { request: Pick<VisualizationViewRequest, "colorCriterion"> } | undefined,
  text: VizText,
) {
  const [criterion, setCriterion] = useState(TYPE_CRITERION);
  const [patterns, setPatterns] = useState(false);
  const types = useTypeNarrowing(format, criterion);
  const ownerCriterion = owner?.request.colorCriterion ?? NONE;
  if (!catalog?.colorCriteria.includes(TYPE_CRITERION))
    return {
      colorCriterion: ownerCriterion,
      request: { colorCriterion: ownerCriterion },
      narrowing: undefined,
      controls: undefined,
      side: () => undefined,
      chartWidth: (width: number) => width,
    };
  // A histogram has one colour and no legend; a type chosen elsewhere stays inactive.
  if (format === "histogram")
    return {
      colorCriterion: ownerCriterion,
      request: { colorCriterion: ownerCriterion },
      narrowing: types.narrowing,
      controls: undefined,
      side: () => undefined,
      chartWidth: (width: number) => width,
    };
  const beside = (width: number) => width >= LEGEND_BESIDE;
  const colorCriterion = criterion === TYPE_CRITERION ? TYPE_CRITERION : NONE;
  return {
    colorCriterion,
    request: { colorCriterion, ...types.condition },
    narrowing: types.narrowing,
    controls: {
      criterion: (
        // A visible name says what "None" turns off (README 44.1).
        <>
          <Text kind="muted">{text.message("colorBy")}</Text>
          <SegmentedChoice
            label={text.message("colorBy")}
            value={criterion}
            onChange={setCriterion}
            options={[
              { value: NONE, label: text.message("colorNone") },
              { value: TYPE_CRITERION, label: text.message("colorType") },
            ]}
          />
        </>
      ),
      patterns,
      onPatterns: setPatterns,
    },
    side: (width: number) =>
      (beside(width) ? "right" : "below") as "right" | "below",
    chartWidth: (width: number) =>
      beside(width) ? width - LEGEND_SPACE : width,
  };
}
