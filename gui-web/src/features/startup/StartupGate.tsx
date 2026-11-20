import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Action, Icon, LiveStatus } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { AnalysisClient, StartupStatus } from "../../ipc";
import { StartupStatusScreen } from "./StartupStatusScreen";
import { startupCopy } from "./startup-copy";
import { showsNotice, startupMemory, useStartupMemory } from "./startup-memory";
import "./startup-status.css";

/**
 * Startup notice for the first screen (F15 FR4.HD1.gui-web.DD1; design F15
 * README 1.1, 1.2).
 *
 * The top of the screen carries a notice only when the app can start an action
 * for the state: today an unconfirmed Full Disk Access on macOS, with "Open
 * Settings". A grade alone shows nothing here; the support information screen
 * and the dot beside its entry carry it. The notice is one line of an icon, a
 * sentence, the action and a dismiss button, with no table of checks; the dot
 * is the way to the details (design F15 README 1.6). Dismissing it remembers
 * the state key and hands focus to the screen title. The permission-only
 * screen replaces `children` when the host reports that analysis entry is
 * not allowed. A status read that fails shows nothing.
 *
 * The live status region is mounted before the status is read, so the notice
 * text and the result of "Open Settings" are announced as changes. When the
 * window becomes active again the access is checked once more, as the user
 * usually returns from System Settings.
 */
export function StartupGate({
  client,
  children,
}: {
  client: AnalysisClient;
  children: ReactNode;
}) {
  const text = useLocaleText();
  const copy = useMemo(() => startupCopy(text), [text]);
  const state = useStartupMemory();
  const [feedback, setFeedback] = useState<string>();
  const [pending, setPending] = useState(false);
  const line = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let current = true;
    client
      .call("app.startup_status", {})
      .then((next) => {
        if (current) startupMemory.setStatus(next);
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [client]);

  const visible = showsNotice(state);
  const recheck = useCallback(async () => {
    try {
      await client.call("app.startup_access", { action: "recheck" });
      startupMemory.setStatus(await client.call("app.startup_status", {}));
    } catch {
      // The notice stays as it is; the next activation tries again.
    }
  }, [client]);

  useEffect(() => {
    if (!visible) return undefined;
    const onFocus = () => void recheck();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [recheck, visible]);

  const status = state.status;
  if (status && !status.analysisAllowed) {
    return (
      <StartupStatusScreen
        client={client}
        onAllowed={(next: StartupStatus) => startupMemory.setStatus(next)}
      />
    );
  }
  const openSettings = async () => {
    setPending(true);
    try {
      const response = await client.call("app.startup_access", {
        action: "open_privacy_settings",
      });
      setFeedback(
        response.settingsOpened
          ? copy.settingsOpened
          : copy.settingsNotConfirmed,
      );
      startupMemory.setStatus(await client.call("app.startup_status", {}));
    } catch {
      setFeedback(copy.actionFailed);
    } finally {
      setPending(false);
    }
  };
  const dismiss = () => {
    const screen = line.current?.nextElementSibling;
    const title = screen?.querySelector("h1") ?? screen;
    startupMemory.dismiss();
    setFeedback(undefined);
    if (title instanceof HTMLElement) {
      if (!title.hasAttribute("tabindex")) title.tabIndex = -1;
      title.focus({ preventScroll: true });
    }
  };
  return (
    <>
      <div
        ref={line}
        className={visible ? "du-startup-notice" : undefined}
        role={visible ? "region" : undefined}
        aria-label={visible ? copy.bannerHeading : undefined}
      >
        {visible && <Icon name="warning" />}
        <LiveStatus>
          {visible && (feedback ?? copy.noticeFullDiskAccess)}
        </LiveStatus>
        {visible && (
          <>
            <Action
              variant="quiet"
              isPending={pending}
              isDisabled={pending}
              onPress={() => void openSettings()}
            >
              {copy.noticeOpenSettings}
            </Action>
            <Action
              variant="quiet"
              icon="close"
              aria-label={copy.noticeDismiss}
              onPress={dismiss}
            />
          </>
        )}
      </div>
      {children}
    </>
  );
}
