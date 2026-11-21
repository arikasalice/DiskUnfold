import { useEffect, useState } from "react";
import { StackedBar, Text } from "../../components";
import type { StackedBarSlice } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type {
  AnalysisClient,
  SizeValue,
  SpaceTotalSize,
  SpaceTypeSummary,
} from "../../ipc";
import { legendLook } from "../../viz/legend-style";
import { folderTypes, NAMED_TYPES } from "../../viz/type-slices";
import { useProductionVizText } from "../../viz/production-text";
import type { SizeBasisFilter, SizeDisplay } from "./space-format";
import "../../theme/type-summary.css";

/** The class the legend names "type unavailable"; the key text names it otherwise. */
const NO_CLASS = "unclassified";
const OTHER = "other";

type Answer =
  | { state: "pending" }
  | { state: "unavailable" }
  | { state: "ready"; summary: SpaceTypeSummary };

/**
 * What fills the scope, as one line (F06 README 13, F02 #300): the five largest
 * types and the rest as a stacked bar, with the same words under it. The classes
 * come from `space.type_summary`, which counts them as the chart's legend does, so
 * the bar and the legend of the space distribution agree for the same result.
 *
 * The bar is decorative: the lines under it carry the same names and sizes. Nothing
 * is drawn while the answer is awaited or when it cannot be had.
 */
export function TypeSummaryBar({
  client,
  resultVersion,
  basis,
  lead,
  awaited,
  display,
}: {
  client: AnalysisClient;
  resultVersion: string;
  basis: SizeBasisFilter;
  /** The size the card leads with, in the same basis: the whole the bar divides. */
  lead: SpaceTotalSize | undefined;
  /** The lead size is still awaited, so it says nothing about the whole yet. */
  awaited: boolean;
  display: SizeDisplay;
}) {
  const text = useLocaleText();
  const viz = useProductionVizText(display);
  const [answer, setAnswer] = useState<Answer>({ state: "pending" });
  useEffect(() => {
    // Unique physical size is not counted by type, so there is nothing to ask.
    if (basis === "unique_physical") {
      setAnswer({ state: "unavailable" });
      return;
    }
    let current = true;
    setAnswer({ state: "pending" });
    // A client that throws instead of rejecting leaves the card empty as well.
    new Promise<SpaceTypeSummary>((resolve) =>
      resolve(client.call("space.type_summary", { resultVersion, basis })),
    )
      .then((summary) => {
        if (current && summary.resultVersion === resultVersion)
          setAnswer({ state: "ready", summary });
      })
      .catch(() => {
        if (current) setAnswer({ state: "unavailable" });
      });
    return () => {
      current = false;
    };
  }, [client, resultVersion, basis]);
  if (answer.state !== "ready") return null;
  const { summary } = answer;
  const title = text("ui.analysis.type_summary_title", "What fills this scope");
  if (summary.slices.length === 0) {
    // An empty list is a scope with no file only when every range was read.
    return summary.lowerBound ? (
      <section className="du-type-summary" aria-label={title}>
        <Text kind="muted">
          {title} · {text("ui.analysis.unavailable", "Unavailable")}
        </Text>
      </section>
    ) : null;
  }
  const unknownSize: SizeValue = {
    basis,
    evidence: "unknown",
    state: "unknown",
  };
  const whole = (!awaited && lead?.size) || unknownSize;
  const types = folderTypes(summary.slices, whole, summary.legendOrder, {
    message: viz.message,
    quantity: viz.quantity,
    className: (key) =>
      viz.key(
        key === NO_CLASS ? "ui.visualization.legend.type.unavailable" : key,
      ),
  });
  if (types === null) return null;
  const named = new Set(
    summary.slices.slice(0, NAMED_TYPES).map((slice) => slice.classKey),
  );
  const restBytes = summary.slices
    .filter((slice) => !named.has(slice.classKey))
    .reduce(
      (sum, slice) =>
        sum + Number(slice.total.state === "unknown" ? 0 : slice.total.value),
      0,
    );
  const slices: StackedBarSlice[] = [
    ...types.bar.filter((slice) => named.has(slice.key)),
    ...(restBytes > 0
      ? [
          {
            key: OTHER,
            ...legendLook("type.unclassified", []),
            tone: "neutral",
            pattern: null,
            patternAlways: false,
            value: restBytes,
          },
        ]
      : []),
  ];
  return (
    <section className="du-type-summary" aria-label={title}>
      <Text kind="muted">{title}</Text>
      <StackedBar
        slices={slices}
        height={12}
        {...(whole.state === "exact" && types.total !== undefined
          ? { total: types.total }
          : {})}
      />
      <Text kind="muted">{types.said.join(" · ")}</Text>
    </section>
  );
}
