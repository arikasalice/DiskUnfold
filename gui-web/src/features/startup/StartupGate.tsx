import { useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Action, LiveStatus, Notice, Readout } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { AnalysisClient, StartupStatus } from "../../ipc";
import { StartupStatusScreen } from "./StartupStatusScreen";
import { checkLabel, outcomeLabel, startupCopy } from "./startup-copy";
import type { StartupCopy } from "./startup-copy";
import "./startup-status.css";

function bannerMessage(status: StartupStatus, copy: StartupCopy) {
  switch (status.grade) {
    case "limited":
      return copy.bannerLimited;
    case "outside_verified":
      return copy.bannerOutsideVerified;
    case "unverifiable":
      return copy.bannerUnverifiable;
    default:
      return copy.bannerUnknown;
  }
}

/**
 * Startup notice for the first screen (F15 FR4.HD1.gui-web.DD1).
 *
 * Shows a dismissible, non-blocking banner above `children` when the startup
 * grade is not a pass. The permission-only screen replaces `children` only when
 * the host reports that analysis entry is not allowed. A status read that fails
 * shows nothing: the banner is advisory and never gates the first screen.
 * Dismissal lasts for this session only and hands keyboard focus to the first
 * screen, so the focused button's removal does not drop focus to the page.
 *
 * The live status region is mounted before the status is read and receives the
 * notice text afterwards, so assistive technology announces the change.
 * `onOpenSupport` lets the shell open the support information screen from
 * the banner; this feature does not know where that screen lives.
 */
export function StartupGate({
  client,
  children,
  onOpenSupport,
}: {
  client: AnalysisClient;
  children: ReactNode;
  onOpenSupport?: () => void;
}) {
  const text = useLocaleText();
  const copy = useMemo(() => startupCopy(text), [text]);
  const [status, setStatus] = useState<StartupStatus>();
  const [dismissed, setDismissed] = useState(false);
  const banner = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let current = true;
    client
      .call("app.startup_status", {})
      .then((next) => {
        if (current) setStatus(next);
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [client]);

  if (status && !status.analysisAllowed) {
    return <StartupStatusScreen client={client} />;
  }
  const dismiss = () => {
    const next = banner.current?.nextElementSibling;
    setDismissed(true);
    if (next instanceof HTMLElement) {
      if (!next.hasAttribute("tabindex")) next.tabIndex = -1;
      next.focus({ preventScroll: true });
    }
  };
  const rows =
    status?.checks.filter(
      (check) =>
        check.check !== "full_disk_access" &&
        check.result !== "pass" &&
        check.result !== "not_applicable",
    ) ?? [];
  const visible = status !== undefined && status.grade !== "pass" && !dismissed;
  return (
    <>
      <div
        ref={banner}
        className={visible ? "du-startup-banner" : undefined}
        role={visible ? "region" : undefined}
        aria-label={visible ? copy.bannerHeading : undefined}
      >
        <LiveStatus>
          {visible && (
            <Notice announce={false}>{bannerMessage(status, copy)}</Notice>
          )}
        </LiveStatus>
        {visible && (
          <div className="du-startup-banner__details">
            <Readout
              label={copy.checksHeading}
              rows={rows.map((check) => ({
                label: checkLabel(check, copy),
                value: outcomeLabel(check, copy),
                wideValue: true,
              }))}
            />
            <div className="du-startup__actions">
              {onOpenSupport && (
                <Action variant="quiet" onPress={onOpenSupport}>
                  {copy.bannerOpenSupport}
                </Action>
              )}
              <Action variant="quiet" onPress={dismiss}>
                {copy.bannerDismiss}
              </Action>
            </div>
          </div>
        )}
      </div>
      {children}
    </>
  );
}
