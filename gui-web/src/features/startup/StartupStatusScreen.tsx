import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Action,
  Heading,
  LiveStatus,
  Notice,
  Progress,
  Readout,
  Text,
  useTitleBarContext,
} from "../../components";
import type { AnalysisClient, StartupStatus } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import {
  checkRows,
  startupCopy,
  startupNotice,
  statusRows,
} from "./startup-copy";
import "./startup-status.css";

/** How often the permission is read again while this screen is shown. */
export const PERMISSION_POLL_INTERVAL_MS = 2000;

/** True once the permission is known and analysis entry is open, so the screen can be left. */
function permissionSettled(status: StartupStatus) {
  return status.analysisAllowed && status.access !== "unknown";
}

/**
 * Permission screen (F15 FR4.HD1.gui-web.DD1, IF-05, IF-20).
 *
 * The screen reads the permission again every {@link PERMISSION_POLL_INTERVAL_MS} and
 * whenever the window regains focus or becomes visible, because the user usually returns
 * from System Settings. One poll runs at a time, and a reply that is no longer the latest
 * request is dropped. When access is granted and analysis is allowed, polling stops and
 * `onAllowed` receives the new status so the shell can show the main screen.
 */
export function StartupStatusScreen({
  client,
  onAllowed,
}: {
  client: AnalysisClient;
  onAllowed?: (status: StartupStatus) => void;
}) {
  const text = useLocaleText();
  const copy = useMemo(() => startupCopy(text), [text]);
  useTitleBarContext(copy.title);
  const [status, setStatus] = useState<StartupStatus>();
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [actionFailed, setActionFailed] = useState(false);
  const [pending, setPending] = useState(false);
  const [feedback, setFeedback] = useState<string>();
  const latestRequest = useRef(0);
  const pollRunning = useRef(false);
  const mounted = useRef(true);
  const onAllowedRef = useRef(onAllowed);
  onAllowedRef.current = onAllowed;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const accept = useCallback((next: StartupStatus) => {
    setStatus(next);
    if (permissionSettled(next)) onAllowedRef.current?.(next);
  }, []);

  const loadStatus = useCallback(async () => {
    const request = ++latestRequest.current;
    setLoading(true);
    setLoadFailed(false);
    try {
      const next = await client.call("app.startup_status", {});
      if (mounted.current && request === latestRequest.current) accept(next);
    } catch {
      if (mounted.current && request === latestRequest.current) {
        setLoadFailed(true);
      }
    } finally {
      if (mounted.current && request === latestRequest.current) {
        setLoading(false);
      }
    }
  }, [accept, client]);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  const poll = useCallback(async () => {
    if (pollRunning.current) return;
    pollRunning.current = true;
    const request = ++latestRequest.current;
    try {
      await client.call("app.startup_access", { action: "recheck" });
      const next = await client.call("app.startup_status", {});
      if (mounted.current && request === latestRequest.current) accept(next);
    } catch {
      // The next tick or focus event tries again; a failed poll shows nothing.
    } finally {
      pollRunning.current = false;
    }
  }, [accept, client]);

  const waiting = status !== undefined && !permissionSettled(status);
  useEffect(() => {
    if (!waiting) return undefined;
    const timer = window.setInterval(
      () => void poll(),
      PERMISSION_POLL_INTERVAL_MS,
    );
    const onFocus = () => void poll();
    const onVisibility = () => {
      if (document.visibilityState === "visible") void poll();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [poll, waiting]);

  const openSettings = useCallback(async () => {
    const request = ++latestRequest.current;
    setPending(true);
    setActionFailed(false);
    setFeedback(undefined);
    try {
      const response = await client.call("app.startup_access", {
        action: "open_privacy_settings",
      });
      if (mounted.current && request === latestRequest.current) {
        setFeedback(
          response.settingsOpened
            ? copy.settingsOpened
            : copy.settingsNotConfirmed,
        );
      }
      const refreshed = await client.call("app.startup_status", {});
      if (mounted.current && request === latestRequest.current) {
        accept(refreshed);
      }
    } catch {
      if (mounted.current) setActionFailed(true);
    } finally {
      if (mounted.current) setPending(false);
    }
  }, [accept, client, copy]);

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

  // Full Disk Access is a macOS permission. Where the host reports it as not applicable
  // (Windows, Linux) the screen shows no permission guidance, rows or settings action.
  const permissionApplies = status.access !== "not_applicable";
  const locationNotice = permissionApplies &&
    status.appLocation === "elsewhere" &&
    status.access !== "confirmed" && (
      <Notice tone="info">{copy.moveToApplications}</Notice>
    );
  const notice = startupNotice(status, copy);

  return (
    <main className="du-app du-startup-screen">
      <header className="du-startup__header">
        <Heading level={1}>{copy.title}</Heading>
        <Text>
          {permissionApplies ? copy.introduction : copy.introductionOther}
        </Text>
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
          {locationNotice}
          <Readout
            label={copy.statusHeading}
            rows={statusRows(status, copy, permissionApplies)}
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
            rows={checkRows(status, copy, permissionApplies)}
          />
        </section>
      </div>
      {actionFailed && <Notice tone="error">{copy.actionFailed}</Notice>}
      {feedback && <LiveStatus>{feedback}</LiveStatus>}
      {permissionApplies && (
        <div className="du-startup__actions">
          <Action
            variant="primary"
            isPending={pending}
            isDisabled={loading || pending}
            onPress={() => void openSettings()}
          >
            {copy.openSettings}
          </Action>
        </div>
      )}
    </main>
  );
}
