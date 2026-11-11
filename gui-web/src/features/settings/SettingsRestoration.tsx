import { useEffect, useState } from "react";
import {
  Action,
  DetailDisclosure,
  Heading,
  InspectionDialog,
  Notice,
  Progress,
  Readout,
  StatusBadge,
  Text,
} from "../../components";
import "../../theme/settings-restoration.css";
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
  const restored = report.items.some((item) => item.source === "restored");
  const defaults = report.items.some((item) => item.source === "default");
  const issues = report.items.some((item) => Boolean(item.reasonKey));
  return (
    <div className="du-settings-restore">
      {report.writeLocked && (
        <Notice tone="error">
          Settings editing is locked for this startup.
        </Notice>
      )}
      {issues && (
        <Notice tone="error">
          Some settings could not be restored as saved. Review the reasons
          below.
        </Notice>
      )}
      <section
        className="du-settings-restore__summary"
        aria-label="Restoration summary"
      >
        <Heading level={3}>Restoration summary</Heading>
        <div className="du-settings-restore__badges">
          {restored && <StatusBadge tone="good">Settings restored</StatusBadge>}
          {defaults && <StatusBadge tone="info">Defaults in use</StatusBadge>}
          {issues && <StatusBadge tone="warning">Issues reported</StatusBadge>}
          {report.writeLocked && (
            <StatusBadge tone="warning">Editing locked</StatusBadge>
          )}
        </div>
        {report.items.length === 0 && (
          <Text>No individual settings were reported.</Text>
        )}
      </section>
      {report.items.length > 0 && (
        <section
          className="du-settings-restore__items"
          aria-label="Individual settings"
        >
          <Heading level={3}>Individual settings</Heading>
          <Readout
            label="Restored settings"
            rows={report.items.map(({ key, source, reasonKey }) => ({
              label: names.get(key) ?? key,
              value: [
                source === "restored" ? "Restored" : "Using default",
                reasonKey &&
                  (reasons.get(reasonKey) ??
                    "A restoration issue was reported."),
              ]
                .filter(Boolean)
                .join(" "),
            }))}
          />
        </section>
      )}
      <DetailDisclosure title="Format details">
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
      </DetailDisclosure>
    </div>
  );
}

type LoadState =
  | { client: AnalysisClient; status: "loading" }
  | { client: AnalysisClient; status: "error" }
  | { client: AnalysisClient; status: "ready"; report: SettingsRestoreStatus };

/** Reads the startup outcome and allows a failed read to be retried. */
export function SettingsRestoration({
  client,
  visible,
}: {
  client: AnalysisClient;
  visible: boolean;
}) {
  const [loaded, setLoaded] = useState<LoadState>();
  const [attempt, setAttempt] = useState(0);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    let active = true;
    void client.call("settings.restore_status", {}).then(
      (report) => {
        if (active) setLoaded({ client, status: "ready", report });
      },
      () => {
        if (active) setLoaded({ client, status: "error" });
      },
    );
    return () => {
      active = false;
    };
  }, [client, attempt]);

  if (!visible) return null;
  const current = loaded?.client === client ? loaded : undefined;
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
          {!current || current.status === "loading" ? (
            <div className="du-settings-restore">
              <Text>Loading restoration status…</Text>
              <Progress label="Loading restoration status" />
            </div>
          ) : current.status === "error" ? (
            <div className="du-settings-restore">
              <Notice tone="error">
                Settings restoration status could not be loaded.
              </Notice>
              <Action
                onPress={() => {
                  setLoaded({ client, status: "loading" });
                  setAttempt((value) => value + 1);
                }}
              >
                Retry
              </Action>
            </div>
          ) : (
            <RestoreReport report={current.report} />
          )}
        </InspectionDialog>
      )}
    </>
  );
}
