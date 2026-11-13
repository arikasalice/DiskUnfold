import { Notice, Readout, StatusBadge } from "../../components";
import type { SupportFeatureAssessment } from "../../ipc";
import {
  areaLabel,
  capabilityLabel,
  differsFromTable,
  reasonText,
  statusLabel,
} from "./support-copy";
import type { SupportCopy } from "./support-copy";

function capabilityTone(state: string) {
  if (state === "supported") return "good" as const;
  if (state === "limited" || state === "unsupported") return "warning" as const;
  return "neutral" as const;
}

function rowFor(feature: SupportFeatureAssessment, copy: SupportCopy) {
  const known = Object.hasOwn(copy.areas, feature.area);
  const differs = differsFromTable(
    feature.supportTableStatus,
    feature.currentCapability,
  );
  const notes = [
    copy.tableLabel + ": " + statusLabel(feature.supportTableStatus, copy),
    ...feature.conditions.map(
      (condition) => copy.conditionLabel + ": " + condition,
    ),
    ...(differs ? [copy.mismatch] : []),
    ...(feature.reasonKey ? [reasonText(feature.reasonKey, copy)] : []),
  ];
  return {
    label: known
      ? areaLabel(feature.area, copy)
      : copy.areaOther + " (" + feature.area + ")",
    value: (
      <StatusBadge tone={capabilityTone(feature.currentCapability)}>
        {copy.nowLabel +
          ": " +
          capabilityLabel(feature.currentCapability, copy)}
      </StatusBadge>
    ),
    emphasized: differs,
    notes,
  };
}

/**
 * Per-area status. The current capability is what the computer does now, so
 * it is the headline value; the support-table status is kept beside it and a
 * difference between the two is called out in words.
 */
export function FeatureAreas({
  features,
  copy,
}: {
  features: readonly SupportFeatureAssessment[];
  copy: SupportCopy;
}) {
  if (features.length === 0) {
    return <Notice tone="info">{copy.featuresNone}</Notice>;
  }
  return (
    <Readout
      label={copy.featuresHeading}
      rows={features.map((feature) => rowFor(feature, copy))}
    />
  );
}
