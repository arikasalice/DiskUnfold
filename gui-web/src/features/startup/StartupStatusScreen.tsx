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
  StartupStatus,
} from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import {
  accessLabel,
  checkLabel,
  gradeLabel,
  outcomeLabel,
  startupCopy,
} from "./startup-copy";
import "./startup-status.css";

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
