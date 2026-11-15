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
import {
  displayMeasurement,
  displayMeasurementLimit,
  displaySize,
  displayRawSize,
  sizeLabels,
} from "./space-format";
import type { SizeBasisFilter, SizeDisplay } from "./space-format";
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
      label: text("ui.analysis.inaccessible_ranges", "Inaccessible ranges"),
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

/** Displays the selected result without replacing it on later publications. */
export function PinnedResult({
  client,
  total,
  roots,
  preferences,
  displayChoice,
  onDisplayChoice,
  onSizeBasisChange,
  onReopenLatest,
}: {
  client: AnalysisClient;
  total: SpaceTotal;
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
          detail={
            lead
              ? displayMeasurement(lead.size, lead.completeness, text)
              : undefined
          }
        >
          {lead ? (
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
            const value = displaySize(size, display, text);
            const raw = displayRawSize(size, regional, text);
            return {
              label: basisLabels[size.basis],
              value: <Unbroken>{value}</Unbroken>,
              notes: [
                ...(raw && raw !== value ? [raw] : []),
                displayMeasurement(size, completeness, text),
                ...measurementLimits.map(displayMeasurementLimit),
              ],
            };
          })}
        />
      </DetailDisclosure>
      <Text kind="muted">
        {basisLabel} · {display.system.toUpperCase()} · {unit}
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
      <VolumeExplanation
        key={`volume:${total.resultVersion}`}
        client={client}
        resultVersion={total.resultVersion}
        roots={roots}
        display={display}
      />
      <ScanIssues
        client={client}
        resultVersion={total.resultVersion}
        onReopenLatest={onReopenLatest}
      />
      <ScopeDetails
        key={total.resultVersion}
        client={client}
        resultVersion={total.resultVersion}
        multiRoot={roots.length > 1}
        display={display}
      />
    </div>
  );
}

/** Shows only the observed attributes of the pinned scope root. */
function ScopeDetails({
  client,
  resultVersion,
  multiRoot,
  display,
}: {
  client: AnalysisClient;
  resultVersion: string;
  multiRoot: boolean;
  display: SizeDisplay;
}) {
  const [open, setOpen] = useState(false);
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
      <Action ref={trigger} onPress={() => setOpen(true)}>
        {text("ui.analysis.view_folder_details", "View folder details")}
      </Action>
      {open && (
        <InspectionDialog
          title={title}
          onClose={() => {
            restore.current = true;
            setOpen(false);
          }}
        >
          <FileInspection
            client={client}
            item={{ resultVersion, itemNo: 0 }}
            itemKind="folder"
            display={display}
          />
        </InspectionDialog>
      )}
    </section>
  );
}
