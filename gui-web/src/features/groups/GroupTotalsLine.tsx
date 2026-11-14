import {
  Action,
  Metric,
  Notice,
  Progress,
  Readout,
  Text,
} from "../../components";
import type { SizeDisplay } from "../../i18n/bytes";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { GroupRatio, GroupTotal, GroupTotals } from "../../ipc";
import type { ReadState, TotalsKey } from "../../state/groups-details";
import { basisLabel, percentText, sizeValueText } from "./group-format";
import { reasonLabel } from "./group-text";

function ratioValue(ratio: GroupRatio, text: LocaleText): string {
  if (ratio.state === "undefined")
    return text(
      "ui.groups.ratio_unavailable",
      "Cannot be calculated. {reason}",
      {
        reason: reasonLabel(ratio.reasonKey, text),
      },
    );
  return (
    percentText(ratio.numerator, ratio.denominator, text) ??
    text("ui.groups.ratio_unavailable", "Cannot be calculated. {reason}", {
      reason: reasonLabel("group.ratio_undefined", text),
    })
  );
}

function sizeRows(total: GroupTotal, display: SizeDisplay, text: LocaleText) {
  return total.sizes.map((entry) => ({
    label: basisLabel(entry.size.basis, text),
    value: sizeValueText(entry.size, display, text),
  }));
}

function Ready({
  totals,
  display,
}: {
  totals: GroupTotals;
  display: SizeDisplay;
}) {
  const text = useLocaleText();
  const complete = totals.union.completeness === "complete";
  return (
    <>
      <div className="du-groups-metrics">
        {totals.union.sizes.map((entry, index) => (
          <Metric
            key={entry.size.basis}
            prominent={index === 0}
            label={text("ui.groups.totals_union", "Combined total, {basis}", {
              basis: basisLabel(entry.size.basis, text),
            })}
            detail={
              complete
                ? undefined
                : text(
                    "ui.groups.total_incomplete",
                    "The measurement is not complete.",
                  )
            }
          >
            {sizeValueText(entry.size, display, text)}
          </Metric>
        ))}
      </div>
      <Readout
        label={text("ui.groups.totals_overlap", "Files in several groups")}
        rows={[
          {
            label: text("ui.groups.totals_overlap", "Files in several groups"),
            value: text("ui.groups.items_count", "{count} items", {
              count: String(totals.overlapCount),
            }),
            notes: sizeRows(totals.overlap, display, text).map(
              (row) => `${row.label}: ${row.value}`,
            ),
          },
          ...totals.ratios.map((ratio) => ({
            label: text(
              "ui.groups.ratio_label",
              "Share of the scope, {basis}",
              {
                basis: basisLabel(ratio.basis, text),
              },
            ),
            value: ratioValue(ratio, text),
          })),
        ]}
      />
    </>
  );
}

/** The combined total of all groups; files in several groups are counted once. */
export function GroupTotalsLine({
  state,
  display,
  onRetry,
}: {
  state: ReadState<TotalsKey, GroupTotals>;
  display: SizeDisplay;
  onRetry: () => void;
}) {
  const text = useLocaleText();
  return (
    <section
      className="du-groups-totals"
      aria-label={text("ui.groups.totals_title", "Combined total")}
    >
      {(state.phase === "loading" || state.phase === "idle") && (
        <Progress label={text("ui.groups.totals_loading", "Loading totals…")} />
      )}
      {state.phase === "error" && (
        <Notice tone="error">
          {text("ui.groups.totals_failed", "Could not load the totals.")}{" "}
          {reasonLabel(state.reasonKey, text)}{" "}
          <Action onPress={onRetry}>
            {text("ui.groups.retry", "Try again")}
          </Action>
        </Notice>
      )}
      {state.phase === "ready" && (
        <Ready totals={state.value} display={display} />
      )}
      <Text kind="muted">
        {text(
          "ui.groups.totals_note",
          "The combined total counts files in several groups once. Members always show their original path.",
        )}
      </Text>
    </section>
  );
}
