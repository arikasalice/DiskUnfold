import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Action,
  Heading,
  LiveStatus,
  Notice,
  Progress,
  Readout,
  Text,
} from "../../components";
import type {
  AnalysisClient,
  StartupAccessAction,
  StartupAccessState,
  StartupCheckResult,
  StartupGrade,
  StartupStatus,
} from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import "./startup-status.css";

function startupCopy(text: LocaleText) {
  return {
    title: text("ui.startup.title", "Startup and privacy"),
    introduction: text(
      "ui.startup.introduction",
      "Review startup checks and Full Disk Access for this Mac. Disk analysis is not enabled in this build.",
    ),
    loading: text("ui.startup.loading", "Loading startup checks"),
    loadFailed: text(
      "ui.startup.load_failed",
      "Startup status could not be read. Try again.",
    ),
    actionFailed: text(
      "ui.startup.action_failed",
      "The permission status could not be updated. Try again.",
    ),
    retry: text("ui.startup.retry", "Try again"),
    statusHeading: text("ui.startup.status_heading", "Startup status"),
    checksHeading: text("ui.startup.checks_heading", "Checks"),
    grade: text("ui.startup.grade", "Overall status"),
    access: text("ui.startup.access", "Full Disk Access"),
    analysis: text("ui.startup.analysis", "Analysis entry"),
    allowed: text("ui.startup.allowed", "Allowed by startup checks"),
    blocked: text("ui.startup.blocked", "Blocked by startup checks"),
    deniedNotice: text(
      "ui.startup.denied_notice",
      "Full Disk Access is denied. Open Privacy & Security Settings, add DiskUnfold under Full Disk Access, then check the permission again.",
    ),
    analysisBlockedNotice: text(
      "ui.startup.analysis_blocked_notice",
      "Startup checks do not allow analysis. Review the listed results for details.",
    ),
    unknownNotice: text(
      "ui.startup.unknown_notice",
      "macOS could not confirm Full Disk Access. Unknown access is not treated as a denial. You can open Privacy & Security Settings and check again.",
    ),
    confirmedNotice: text(
      "ui.startup.confirmed_notice",
      "Full Disk Access is confirmed. This build provides startup checks and permission controls only; disk analysis is not enabled.",
    ),
    notApplicableNotice: text(
      "ui.startup.not_applicable_notice",
      "Full Disk Access does not apply to this operation. This build provides startup checks and permission controls only; disk analysis is not enabled.",
    ),
    openSettings: text(
      "ui.startup.open_settings",
      "Open Privacy & Security Settings",
    ),
    recheck: text("ui.startup.recheck", "Check permission again"),
    settingsOpened: text(
      "ui.startup.settings_opened",
      "System Settings reported that its privacy page opened. Grant Full Disk Access to DiskUnfold, then check the permission again.",
    ),
    settingsNotConfirmed: text(
      "ui.startup.settings_not_confirmed",
      "macOS did not confirm that Settings opened. If needed, open System Settings › Privacy & Security › Full Disk Access, add DiskUnfold, then check again.",
    ),
    rechecked: text("ui.startup.rechecked", "Permission status refreshed."),
    unknown: text("ui.startup.unknown", "Unknown"),
    pass: text("ui.startup.pass", "Passed"),
    limited: text("ui.startup.limited", "Limited"),
    outsideVerified: text(
      "ui.startup.outside_verified",
      "Outside verified combinations",
    ),
    unverifiable: text("ui.startup.unverifiable", "Could not verify"),
    notApplicable: text("ui.startup.not_applicable", "Not applicable"),
    denied: text("ui.startup.denied", "Denied"),
    confirmed: text("ui.startup.confirmed", "Confirmed"),
    checkNames: {
      resource_integrity: text(
        "ui.startup.check.resource_integrity",
        "Built-in resources",
      ),
      webview: text("ui.startup.check.webview", "WebView availability"),
      data_format_and_open: text(
        "ui.startup.check.data_format_and_open",
        "Local data and format",
      ),
      temporary_files: text(
        "ui.startup.check.temporary_files",
        "Temporary files",
      ),
      support_matrix: text(
        "ui.startup.check.support_matrix",
        "Verified environment",
      ),
      capability: text("ui.startup.check.capability", "Platform capabilities"),
      full_disk_access: text(
        "ui.startup.check.full_disk_access",
        "Full Disk Access",
      ),
    } as Record<string, string>,
  };
}

type StartupCopy = ReturnType<typeof startupCopy>;

function gradeLabel(grade: StartupGrade, copy: StartupCopy) {
  switch (grade) {
    case "pass":
      return copy.pass;
    case "limited":
      return copy.limited;
    case "outside_verified":
      return copy.outsideVerified;
    case "unverifiable":
      return copy.unverifiable;
    default:
      return copy.unknown;
  }
}

function accessLabel(access: StartupAccessState, copy: StartupCopy) {
  switch (access) {
    case "confirmed":
      return copy.confirmed;
    case "denied":
      return copy.denied;
    case "not_applicable":
      return copy.notApplicable;
    default:
      return copy.unknown;
  }
}

function outcomeLabel(result: StartupCheckResult, copy: StartupCopy) {
  switch (result.result) {
    case "pass":
      return copy.pass;
    case "limited":
      return copy.limited;
    case "outside_verified":
      return copy.outsideVerified;
    case "unverifiable":
      return copy.unverifiable;
    case "not_applicable":
      return copy.notApplicable;
    default:
      return copy.unknown;
  }
}

function checkLabel(check: StartupCheckResult, copy: StartupCopy) {
  return copy.checkNames[check.check] ?? copy.unknown;
}

export function StartupStatusScreen({ client }: { client: AnalysisClient }) {
  const text = useLocaleText();
  const copy = useMemo(() => startupCopy(text), [text]);
  const [status, setStatus] = useState<StartupStatus>();
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [actionFailed, setActionFailed] = useState(false);
  const [pending, setPending] = useState<StartupAccessAction>();
  const [feedback, setFeedback] = useState<string>();

  const loadStatus = useCallback(async () => {
    setLoading(true);
    setLoadFailed(false);
    try {
      setStatus(await client.call("app.startup_status", {}));
    } catch {
      setLoadFailed(true);
    } finally {
      setLoading(false);
    }
  }, [client]);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  const runAccessAction = useCallback(
    async (action: StartupAccessAction) => {
      setPending(action);
      setActionFailed(false);
      setFeedback(undefined);
      try {
        const response = await client.call("app.startup_access", { action });
        if (action === "open_privacy_settings") {
          setFeedback(
            response.settingsOpened
              ? copy.settingsOpened
              : copy.settingsNotConfirmed,
          );
        }
        const refreshed = await client.call("app.startup_status", {});
        setStatus(refreshed);
        if (action === "recheck") setFeedback(copy.rechecked);
      } catch {
        setActionFailed(true);
      } finally {
        setPending(undefined);
      }
    },
    [client, copy],
  );

  if (loading && !status) {
    return (
      <main className="du-app du-startup-screen">
        <Heading level={1}>{copy.title}</Heading>
        <Progress label={copy.loading} />
      </main>
    );
  }

  if (loadFailed && !status) {
    return (
      <main className="du-app du-startup-screen">
        <Heading level={1}>{copy.title}</Heading>
        <Notice tone="error">{copy.loadFailed}</Notice>
        <div className="du-startup__actions">
          <Action variant="primary" onPress={() => void loadStatus()}>
            {copy.retry}
          </Action>
        </div>
      </main>
    );
  }

  if (!status) return null;

  const notice =
    status.access === "denied"
      ? copy.deniedNotice
      : !status.analysisAllowed
        ? copy.analysisBlockedNotice
        : status.access === "confirmed"
          ? copy.confirmedNotice
          : status.access === "not_applicable"
            ? copy.notApplicableNotice
            : copy.unknownNotice;

  return (
    <main className="du-app du-startup-screen">
      <header className="du-startup__header">
        <Heading level={1}>{copy.title}</Heading>
        <Text>{copy.introduction}</Text>
      </header>
      <div className="du-startup__panels">
        <section
          className="du-startup__panel"
          aria-labelledby="du-startup-status"
        >
          <Heading level={2} id="du-startup-status">
            {copy.statusHeading}
          </Heading>
          <Notice tone={!status.analysisAllowed ? "error" : "info"}>
            {notice}
          </Notice>
          <Readout
            label={copy.statusHeading}
            rows={[
              { label: copy.grade, value: gradeLabel(status.grade, copy) },
              { label: copy.access, value: accessLabel(status.access, copy) },
              {
                label: copy.analysis,
                value: status.analysisAllowed ? copy.allowed : copy.blocked,
              },
            ]}
          />
        </section>
        <section
          className="du-startup__panel"
          aria-labelledby="du-startup-checks"
        >
          <Heading level={2} id="du-startup-checks">
            {copy.checksHeading}
          </Heading>
          <Readout
            label={copy.checksHeading}
            rows={status.checks.map((check, index) => ({
              label: checkLabel(check, copy),
              value: outcomeLabel(check, copy),
              wideValue: true,
              emphasized:
                check.check === "full_disk_access" &&
                check.result === "blocked",
              key: `${check.check}-${index}`,
            }))}
          />
        </section>
      </div>
      {actionFailed && <Notice tone="error">{copy.actionFailed}</Notice>}
      {feedback && <LiveStatus>{feedback}</LiveStatus>}
      <div className="du-startup__actions">
        <Action
          variant="primary"
          isPending={pending === "open_privacy_settings"}
          isDisabled={loading || pending !== undefined}
          onPress={() => void runAccessAction("open_privacy_settings")}
        >
          {copy.openSettings}
        </Action>
        <Action
          isPending={pending === "recheck"}
          isDisabled={loading || pending !== undefined}
          onPress={() => void runAccessAction("recheck")}
        >
          {copy.recheck}
        </Action>
      </div>
    </main>
  );
}
