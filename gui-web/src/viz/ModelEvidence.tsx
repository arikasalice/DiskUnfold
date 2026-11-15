import { DetailDisclosure, Notice, Readout } from "../components";
import type { ModelResponse } from "./model-adapter";
import { visualizationReason } from "./messages";
import { rawSourceSize } from "./model-text";
import type { VizText } from "./model-text";

/** Measurement basis, totals, source sizes and coverage of one committed model. */
export function ModelEvidence({
  frame,
  text,
}: {
  frame: ModelResponse;
  text: VizText;
}) {
  const { model } = frame;
  const totals = [
    [text.message("scopeTotal"), model.totals.scope],
    [text.message("drawnTotal"), model.totals.drawn],
    [text.message("groupedTotal"), model.totals.grouped],
    [text.message("filteredOutTotal"), model.totals.filteredOut],
  ] as const;
  return (
    <DetailDisclosure title={text.message("measurementBasisTotals")}>
      <Readout
        label={text.message("representation")}
        rows={[
          {
            label: text.message("format"),
            value: text.key(model.representation.format),
          },
          {
            label: text.message("measure"),
            value: text.key(model.representation.measure),
          },
          {
            label: text.message("sizeBasis"),
            value: text.key(model.representation.sizeBasis),
          },
          {
            label: text.message("denominator"),
            value: `${text.quantity(model.denominator.value)} · ${text.key(model.denominator.basis)}`,
          },
        ]}
      />
      <Readout
        label={text.message("independentTotals")}
        rows={totals.map(([label, total]) => ({
          label,
          value: text.quantity(total.quantity),
        }))}
      />
      <Readout
        label={text.message("sourceSizes")}
        rows={totals.map(([label, total]) => ({
          label,
          value:
            total.sourceSize === null
              ? text.message("noSourceSize")
              : rawSourceSize(total.sourceSize, text.message),
        }))}
      />
      <Readout
        label={text.message("coverage")}
        rows={[
          {
            label: text.message("groupedItems"),
            value: text.quantity(model.summary.groupedItems),
          },
          {
            label: text.message("omittedItems"),
            value: text.quantity(model.summary.omittedItems),
          },
          {
            label: text.message("unknownItems"),
            value: text.quantity(model.summary.unknownItems),
          },
        ]}
      />
      {model.summary.reasonKeys.map((key) => (
        <Notice key={key}>{text.key(key)}</Notice>
      ))}
      {model.staleness.reasonKey && (
        <Notice>
          {visualizationReason(model.staleness.reasonKey, text.message)}
        </Notice>
      )}
    </DetailDisclosure>
  );
}
