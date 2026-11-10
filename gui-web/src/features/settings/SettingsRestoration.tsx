import { useEffect, useState } from "react";
import { Action, InspectionDialog, Notice, Readout } from "../../components";
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

function RestoreReport({ report }: { report: SettingsRestoreStatus }) {
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

/** Reads the immutable startup outcome once and keeps it across screen changes. */
export function SettingsRestoration({
  client,
  visible,
}: {
  client: AnalysisClient;
  visible: boolean;
}) {
  const [loaded, setLoaded] = useState<{
    client: AnalysisClient;
    report: SettingsRestoreStatus;
  }>();
  const [open, setOpen] = useState(false);
  useEffect(() => {
    let active = true;
    void client.call("settings.restore_status", {}).then(
      (report) => {
        if (active) setLoaded({ client, report });
      },
      () => undefined,
    );
    return () => {
      active = false;
    };
  }, [client]);

  if (!visible || loaded?.client !== client) return null;
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
          <RestoreReport report={loaded.report} />
        </InspectionDialog>
      )}
    </>
  );
}
