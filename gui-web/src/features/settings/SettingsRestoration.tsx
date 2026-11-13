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
import { useLocaleText } from "../../i18n/presentation";

const names = new Map<string, readonly [string, string]>([
  ["a11y.app_scale", ["ui.settings.app_scale", "App scale"]],
  ["a11y.motion", ["ui.settings.motion", "Motion"]],
  ["display.size_unit", ["ui.settings.size_unit", "Size unit"]],
  ["display.size_unit_system", ["ui.settings.unit_system", "Unit system"]],
  ["locale.date_format", ["ui.settings.date_format", "Date format"]],
  [
    "locale.display_language",
    ["ui.settings.display_language", "Display language"],
  ],
  ["locale.number_format", ["ui.settings.number_format", "Number format"]],
  [
    "locale.time_zone_policy",
    ["ui.settings.time_zone_policy", "Time zone policy"],
  ],
  [
    "scope.remember_recent",
    ["ui.settings.remember_recent_scopes", "Remember recent scan scopes"],
  ],
  [
    "scope.recent_limit",
    ["ui.settings.recent_scope_limit", "Recent scope limit"],
  ],
  [
    "space.default_basis",
    ["ui.settings.default_size_basis", "Default size basis"],
  ],
  [
    "space.shared_table_max_entries",
    ["ui.settings.file_identity_limit", "File identity limit"],
  ],
]);

const reasons = new Map<string, readonly [string, string]>([
  [
    "settings.value_invalid",
    ["ui.settings.reason_value_invalid", "The saved value failed validation."],
  ],
  [
    "settings.restore_failed",
    [
      "ui.settings.reason_restore_failed",
      "Stored settings could not be fully restored.",
    ],
  ],
  [
    "settings.schema_newer",
    [
      "ui.settings.reason_schema_newer",
      "The saved settings format is newer than this app.",
    ],
  ],
  [
    "settings.schema_unsupported",
    [
      "ui.settings.reason_schema_unsupported",
      "The saved settings format is unsupported.",
    ],
  ],
]);

function RestoreReport({ report }: { report: SettingsRestoreStatus }) {
  const text = useLocaleText();
  const restored = report.items.some((item) => item.source === "restored");
  const defaults = report.items.some((item) => item.source === "default");
  const issues = report.items.some((item) => Boolean(item.reasonKey));
  let unknownSetting = 0;
  return (
    <div className="du-settings-restore">
      {report.writeLocked && (
        <Notice tone="error">
          {text(
            "ui.settings.editing_locked_startup",
            "Settings editing is locked for this startup.",
          )}
        </Notice>
      )}
      {issues && (
        <Notice tone="error">
          {text(
            "ui.settings.restore_issues_review",
            "Some settings could not be restored as saved. Review the reasons below.",
          )}
        </Notice>
      )}
      <section
        className="du-settings-restore__summary"
        aria-label={text(
          "ui.settings.restoration_summary",
          "Restoration summary",
        )}
      >
        <Heading level={3}>
          {text("ui.settings.restoration_summary", "Restoration summary")}
        </Heading>
        <div className="du-settings-restore__badges">
          {restored && (
            <StatusBadge tone="good">
              {text("ui.settings.restored", "Settings restored")}
            </StatusBadge>
          )}
          {defaults && (
            <StatusBadge tone="info">
              {text("ui.settings.defaults_in_use", "Defaults in use")}
            </StatusBadge>
          )}
          {issues && (
            <StatusBadge tone="warning">
              {text("ui.settings.issues_reported", "Issues reported")}
            </StatusBadge>
          )}
          {report.writeLocked && (
            <StatusBadge tone="warning">
              {text("ui.settings.editing_locked", "Editing locked")}
            </StatusBadge>
          )}
        </div>
        {report.items.length === 0 && (
          <Text>
            {text(
              "ui.settings.no_individual_settings",
              "No individual settings were reported.",
            )}
          </Text>
        )}
      </section>
      {report.items.length > 0 && (
        <section
          className="du-settings-restore__items"
          aria-label={text(
            "ui.settings.individual_settings",
            "Individual settings",
          )}
        >
          <Heading level={3}>
            {text("ui.settings.individual_settings", "Individual settings")}
          </Heading>
          <Readout
            label={text("ui.settings.restored_settings", "Restored settings")}
            rows={report.items.map(({ key, source, reasonKey }) => {
              const name = names.get(key);
              const reason = reasonKey ? reasons.get(reasonKey) : undefined;
              return {
                label: name
                  ? text(name[0], name[1])
                  : text(
                      "ui.settings.unrecognized_setting",
                      "Unrecognized setting {number}",
                      { number: BigInt(++unknownSetting) },
                    ),
                value: [
                  source === "restored"
                    ? text("ui.settings.item_restored", "Restored")
                    : text("ui.settings.using_default", "Using default"),
                  reasonKey &&
                    (reason
                      ? text(reason[0], reason[1])
                      : text(
                          "ui.settings.reason_generic",
                          "A restoration issue was reported.",
                        )),
                ]
                  .filter(Boolean)
                  .join(" "),
              };
            })}
          />
        </section>
      )}
      <DetailDisclosure
        title={text("ui.settings.format_details", "Format details")}
      >
        <Readout
          label={text("ui.settings.settings_format", "Settings format")}
          rows={[
            {
              label: text(
                "ui.settings.saved_format_version",
                "Saved format version",
              ),
              value:
                report.storedSchema ??
                text("ui.settings.unavailable", "Unavailable"),
            },
            {
              label: text(
                "ui.settings.app_format_version",
                "App format version",
              ),
              value: report.appSchema,
            },
            {
              label: text("ui.settings.migration", "Migration"),
              value: report.migrated
                ? text("ui.settings.migration_applied", "Applied")
                : text("ui.settings.migration_not_applied", "Not applied"),
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
  const text = useLocaleText();
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
        {text("ui.settings.restoration", "Settings restoration")}
      </Action>
      {open && (
        <InspectionDialog
          title={text("ui.settings.restoration", "Settings restoration")}
          closeLabel={text("ui.settings.close_details", "Close details")}
          onClose={() => setOpen(false)}
        >
          {!current || current.status === "loading" ? (
            <div className="du-settings-restore">
              <Text>
                {text(
                  "ui.settings.loading_restoration",
                  "Loading restoration status…",
                )}
              </Text>
              <Progress
                label={text(
                  "ui.settings.loading_restoration_progress",
                  "Loading restoration status",
                )}
              />
            </div>
          ) : current.status === "error" ? (
            <div className="du-settings-restore">
              <Notice tone="error">
                {text(
                  "ui.settings.restoration_load_failed",
                  "Settings restoration status could not be loaded.",
                )}
              </Notice>
              <Action
                onPress={() => {
                  setLoaded({ client, status: "loading" });
                  setAttempt((value) => value + 1);
                }}
              >
                {text("ui.settings.retry", "Retry")}
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
