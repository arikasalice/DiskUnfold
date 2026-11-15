import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useLocaleText } from "../../i18n/presentation";
import { displayInteger, useRegionalFormat } from "../../i18n/regional-format";
import {
  Action,
  DetailDisclosure,
  InspectionDialog,
  Metric,
  NameText,
  Notice,
  Readout,
  Text,
  Unbroken,
} from "../../components";
import { SIZE_UNIT_RULES } from "../../ipc";
import type { AnalysisClient, OsPath, SpaceTotal } from "../../ipc";
import { ScanIssues } from "./ScanIssues";
import { FileInspection } from "./FileInspection";
import { SizeDisplayControls } from "./SizeDisplayControls";
import { VolumeExplanation } from "./VolumeExplanation";
import type { VolumePart } from "./summary-batch";
import { useSizeWait } from "./awaited-sizes";
import {
  awaitedSize,
  displayAwaitedSize,
  displayMeasurement,
  displayMeasurementLimit,
  displaySize,
  displayRawSize,
  displayUnitSystem,
  sizeLabels,
} from "./space-format";
import type { SizeBasisFilter, SizeDisplay, SizeWait } from "./space-format";
import { useDialogResult } from "./use-dialog-result";
import { DialogUpdateStatus } from "./DialogUpdateStatus";
import type { AnalysisDisplayPreferences } from "./use-display-preferences";

function Coverage({ total }: { total: SpaceTotal }) {
  const regional = useRegionalFormat();
  const text = useLocaleText();
  const counters = [
    {
      label: text("ui.analysis.missing_items", "Missing items"),
      count: total.missingItems,
    },
    {
      label: text("ui.analysis.inaccessible_ranges", "Inaccessible locations"),
      count: total.inaccessibleRanges,
    },
    {
      label: text("ui.analysis.untraversed_links", "Untraversed links"),
      count: total.untraversedLinks,
    },
  ];
  return (
    <Readout
      label={text("ui.analysis.coverage_limits", "Coverage limits")}
      rows={counters.map(({ label, count }) => ({
        label,
        value:
          count === null
            ? text("ui.analysis.not_recorded", "Not recorded")
            : displayInteger(BigInt(count), regional),
      }))}
    />
  );
}

/** The analysis roots in order, each drawn as a path whose last component is a name. */
export function RootPaths({ roots }: { roots: readonly OsPath[] }) {
  return roots.map((root, index) => (
    <Fragment key={index}>
      {index > 0 && ", "}
      <NameText path name={root.display} />
    </Fragment>
  ));
}

/**
 * Displays the given totals in place. A newer `total` changes the values
 * without remounting the parts around them, so expanded sections, focus and
 * open dialogs stay as they are. Expanded parts follow `total`, and `volume`
 * is the volume part read for the same result (see `useSummaryBatch`);
 * dialogs open on `latestVersion`, the newest result known for the scan.
 * `wait` says how `scan`, the scan that still publishes, stands: a logical
 * or allocated size without a value then reads as still to come instead of
 * unavailable, for as long as its results allow (see `useSizeWait`).
 */
export function PinnedResult({
  client,
  total,
  latestVersion,
  volume,
  scan,
  wait,
  roots,
  preferences,
  displayChoice,
  onDisplayChoice,
  onSizeBasisChange,
  onReopenLatest,
}: {
  client: AnalysisClient;
  total: SpaceTotal;
  latestVersion: string;
  volume: VolumePart;
  scan: string | undefined;
  wait: SizeWait | undefined;
  roots: readonly OsPath[];
  preferences: AnalysisDisplayPreferences;
  displayChoice: SizeDisplay | undefined;
  onDisplayChoice: (choice: SizeDisplay) => void;
  onSizeBasisChange?:
    ((resultVersion: string, basis: SizeBasisFilter) => void) | undefined;
  onReopenLatest: () => void;
}) {
  const [basisChoice, setBasis] = useState<SizeBasisFilter>();
  const regional = useRegionalFormat();
  const text = useLocaleText();
  const choice = displayChoice ?? preferences.display;
  const display = regional ? { ...choice, regional } : choice;
  const basis = basisChoice ?? preferences.basis;
  useEffect(() => {
    onSizeBasisChange?.(total.resultVersion, basis);
  }, [basis, onSizeBasisChange, total.resultVersion]);
  const lead = total.sizes.find(({ size }) => size.basis === basis);
  const unavailable = text("ui.analysis.unavailable", "Unavailable");
  const unit =
    display.unit === "auto"
      ? text("ui.analysis.automatic", "Automatic")
      : (SIZE_UNIT_RULES[display.system].symbols[display.unit] ?? unavailable);
  const basisLabels = {
    logical: text("ui.analysis.logical_size", sizeLabels.logical),
    allocated: text("ui.analysis.allocated_size", sizeLabels.allocated),
    unique_physical: text(
      "ui.analysis.unique_physical_size",
      sizeLabels.unique_physical,
    ),
  };
  const basisLabel = basisLabels[basis];
  const pathsLabel = text("ui.analysis.scanned_paths", "Scanned paths");
  const measurements = text("ui.analysis.measurements", "Measurement details");
  const sizeWait = useSizeWait(total, scan, wait);
  const leadWait =
    lead && awaitedSize(lead.size, lead.measurementLimits, sizeWait);
  const leadAwaited = leadWait && displayAwaitedSize(leadWait, text);
  return (
    <div className="du-results">
      <Readout
        label={pathsLabel}
        rows={[
          {
            label: pathsLabel,
            value: <RootPaths roots={roots} />,
          },
        ]}
      />
      <div className="du-metrics">
        <Metric
          label={basisLabel}
          pending={leadAwaited !== undefined}
          detail={
            leadAwaited
              ? leadAwaited.detail
              : lead
                ? displayMeasurement(lead.size, lead.completeness, text)
                : undefined
          }
        >
          {leadAwaited ? (
            leadAwaited.value
          ) : lead ? (
            <Unbroken>{displaySize(lead.size, display, text)}</Unbroken>
          ) : (
            unavailable
          )}
        </Metric>
      </div>
      <Coverage total={total} />
      <DetailDisclosure title={measurements}>
        <Readout
          label={measurements}
          rows={total.sizes.map(({ size, completeness, measurementLimits }) => {
            const waited = awaitedSize(size, measurementLimits, sizeWait);
            // An awaited size shows no measurement and none of its limits.
            if (waited) {
              const awaited = displayAwaitedSize(waited, text);
              return {
                label: basisLabels[size.basis],
                value: awaited.value,
                pending: true,
                notes: [awaited.detail],
              };
            }
            const value = displaySize(size, display, text);
            const raw = displayRawSize(size, regional, text);
            return {
              label: basisLabels[size.basis],
              value: <Unbroken>{value}</Unbroken>,
              notes: [
                ...(raw && raw !== value ? [raw] : []),
                displayMeasurement(size, completeness, text),
                ...measurementLimits.map((key) =>
                  displayMeasurementLimit(key, text),
                ),
              ],
            };
          })}
        />
      </DetailDisclosure>
      <Text kind="muted">
        {basisLabel} · {displayUnitSystem(display.system, text)} · {unit}
      </Text>
      {preferences.warning && (
        <Notice>
          {text(
            "ui.analysis.display_restore_warning",
            "Some display preferences could not be restored. Using the available settings and defaults.",
          )}
        </Notice>
      )}
      <DetailDisclosure
        title={text("ui.analysis.display_options", "Display options")}
      >
        <SizeDisplayControls
          basis={basis}
          display={display}
          onBasis={setBasis}
          onDisplay={onDisplayChoice}
        />
      </DetailDisclosure>
      <VolumeExplanation volume={volume} roots={roots} display={display} />
      <ScanIssues
        client={client}
        resultVersion={total.resultVersion}
        onReopenLatest={onReopenLatest}
      />
      <ScopeDetails
        client={client}
        latestVersion={latestVersion}
        multiRoot={roots.length > 1}
        display={display}
      />
    </div>
  );
}

/**
 * Shows the observed attributes of the scope root in a dialog that keeps the
 * result it opened with (see `useDialogResult`). The scope root keeps its
 * item number in every result, so a copy refused because the result was
 * replaced continues once on the newest result.
 */
function ScopeDetails({
  client,
  latestVersion,
  multiRoot,
  display,
}: {
  client: AnalysisClient;
  latestVersion: string;
  multiRoot: boolean;
  display: SizeDisplay;
}) {
  const dialog = useDialogResult(latestVersion);
  const open = dialog.version !== undefined;
  const text = useLocaleText();
  const title = text("ui.analysis.folder_details", "Folder details");
  const trigger = useRef<HTMLButtonElement>(null);
  const restore = useRef(false);
  useLayoutEffect(() => {
    if (!open && restore.current) {
      trigger.current?.focus();
      restore.current = false;
    }
  }, [open]);

  if (multiRoot) return null;

  return (
    <section aria-label={title}>
      <Action ref={trigger} onPress={dialog.open}>
        {text("ui.analysis.view_folder_details", "View folder details")}
      </Action>
      {dialog.version !== undefined && (
        <InspectionDialog
          title={title}
          onClose={() => {
            restore.current = true;
            dialog.close();
          }}
        >
          <DialogUpdateStatus updated={dialog.updated} />
          <FileInspection
            key={dialog.version}
            client={client}
            item={{ resultVersion: dialog.version, itemNo: 0 }}
            itemKind="folder"
            display={display}
            onStale={dialog.stale}
            copy={{
              pending: dialog.copy,
              refused: dialog.staleCopy,
              done: dialog.copied,
            }}
          />
        </InspectionDialog>
      )}
    </section>
  );
}
