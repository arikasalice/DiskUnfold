import { useEffect, useState } from "react";
import {
  Action,
  InspectionDialog,
  Notice,
  Progress,
  Readout,
} from "../../components";
import type { AnalysisClient, SettingsRestoreStatus } from "../../ipc";

const names = new Map([
  ["a11y.app_scale", "App scale"],
  ["a11y.motion", "Motion"],
  ["locale.date_format", "Date format"],
  ["locale.display_language", "Display language"],
  ["locale.number_format", "Number format"],
  ["locale.time_zone_policy", "Time zone policy"],
  ["scope.remember_recent", "Remember recent scan scopes"],
  ["space.shared_table_max_entries", "File identity limit"],
]);

const reasons = new Map([
  ["settings.value_invalid", "The saved value failed validation."],
  ["settings.restore_failed", "Stored settings could not be fully restored."],
  [
    "settings.schema_newer",
    "The saved settings format is newer than this app.",
  ],
  ["settings.schema_unsupported", "The saved settings format is unsupported."],
]);

function RestoreReport({ client }: { client: AnalysisClient }) {
  const [report, setReport] = useState<SettingsRestoreStatus>();
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let active = true;
    void client.call("settings.restore_status", {}).then(
      (result) => {
        if (active) setReport(result);
      },
      () => {
        if (active) setFailed(true);
      },
    );
    return () => {
      active = false;
    };
  }, [client]);

  if (failed)
    return (
      <Notice tone="error">Settings restoration could not be read.</Notice>
    );
  if (!report) return <Progress label="Reading settings restoration" />;
  return (
    <>
      {report.writeLocked && (
        <Notice tone="error">
          Settings editing is locked for this startup.
        </Notice>
      )}
      <Readout
        label="Settings format"
        rows={[
          {
            label: "Saved format version",
            value: report.storedSchema ?? "Unavailable",
          },
          { label: "App format version", value: report.appSchema },
          {
            label: "Migration",
            value: report.migrated ? "Applied" : "Not applied",
          },
        ]}
      />
      <Readout
        label="Restored settings"
        rows={report.items.map(({ key, source, reasonKey }) => ({
          label: names.get(key) ?? key,
          value: [
            source === "restored" ? "Restored" : "Using default",
            reasonKey &&
              (reasons.get(reasonKey) ?? "A restoration issue was reported."),
          ]
            .filter(Boolean)
            .join(" "),
        }))}
      />
    </>
  );
}

/** Opens the read-only settings outcome for this startup. */
export function SettingsRestoration({ client }: { client: AnalysisClient }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Action variant="quiet" onPress={() => setOpen(true)}>
        Settings restoration
      </Action>
      {open && (
        <InspectionDialog
          title="Settings restoration"
          onClose={() => setOpen(false)}
        >
          <RestoreReport client={client} />
        </InspectionDialog>
      )}
    </>
  );
}
