import { useEffect, useState } from "react";
import { StackedBar, Text } from "../../components";
import type { StackedBarSlice } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type {
  AnalysisClient,
  SizeValue,
  SpaceTotalSize,
  SpaceTypeSummary,
  VizQuantity,
} from "../../ipc";
import { legendLook } from "../../viz/legend-style";
import { NAMED_TYPES } from "../../viz/type-slices";
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

/** One item under the bar: the colour of its slice, and its words. */
type Item = {
  key: string;
  /** A slice tone, or "unsplit" for the part of the whole that no type holds. */
  tone: string;
  words: string;
};

const sizeOf = (value: VizQuantity) =>
  value.state === "unknown" ? 0 : Number(value.value);

const bytes = (value: number, lowerBound: boolean): VizQuantity => ({
  unit: "bytes",
  state: lowerBound ? "lower_bound" : "exact",
  value: String(value),
});

/**
 * What fills the scope, as one line (F06 README 13, F02 #300): the five largest types
 * and the rest as a stacked bar, largest first, and the same content as a list under
 * it, each item led by the colour of its slice. The classes come from
 * `space.type_summary`, which counts them as the chart's legend does, so the bar and
 * the legend of the space distribution agree for the same result.
 *
 * The bar divides the card's size: what no type holds is the empty part of the bar
 * and the last item of the list. The bar is decorative, the list carries the content.
 * Nothing is drawn while the answer is awaited or when it cannot be had.
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
  const whole: SizeValue | undefined = awaited ? undefined : lead?.size;
  const wholeBytes =
    whole === undefined || whole.state === "unknown"
      ? undefined
      : Number(whole.bytes);
  const className = (key: string) =>
    viz.key(
      key === NO_CLASS ? "ui.visualization.legend.type.unavailable" : key,
    );
  const named = summary.slices.slice(0, NAMED_TYPES);
  const rest = summary.slices.slice(NAMED_TYPES);
  const restBytes = rest.reduce((sum, slice) => sum + sizeOf(slice.total), 0);
  const counted = summary.slices.reduce(
    (sum, slice) => sum + sizeOf(slice.total),
    0,
  );
  const lowerBound =
    summary.lowerBound ||
    whole?.state !== "exact" ||
    summary.slices.some((slice) => slice.total.state === "lower_bound");
  const looks = named.map((slice) => ({
    slice,
    look: legendLook(slice.classKey, summary.legendOrder),
  }));
  const items: Item[] = [
    ...looks.map(({ slice, look }) => ({
      key: slice.classKey,
      tone: look.tone,
      words: `${className(slice.classKey)} ${viz.quantity(slice.total)}`,
    })),
    ...(rest.length > 0
      ? [
          {
            key: OTHER,
            tone: "neutral",
            words: text(
              "ui.analysis.type_summary_rest",
              "{count} more types {size}",
              {
                count: BigInt(rest.length),
                size: viz.quantity(bytes(restBytes, lowerBound)),
              },
            ),
          },
        ]
      : []),
    ...(wholeBytes !== undefined && wholeBytes > counted
      ? [
          {
            key: "unsplit",
            tone: "unsplit",
            words: text(
              "ui.analysis.type_summary_unsplit",
              "Not split by type {size}",
              { size: viz.quantity(bytes(wholeBytes - counted, lowerBound)) },
            ),
          },
        ]
      : []),
  ];
  const slices: StackedBarSlice[] = [
    ...looks.map(({ slice, look }) => ({
      key: slice.classKey,
      ...look,
      value: sizeOf(slice.total),
    })),
    ...(restBytes > 0
      ? [
          {
            key: OTHER,
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
        {...(wholeBytes === undefined ? {} : { total: wholeBytes })}
      />
      <div className="du-type-summary__list" role="list">
        {items.map((item) => (
          <div key={item.key} role="listitem">
            <div
              className="du-type-summary__swatch"
              data-tone={item.tone}
              aria-hidden="true"
            />
            {item.words}
          </div>
        ))}
      </div>
    </section>
  );
}
