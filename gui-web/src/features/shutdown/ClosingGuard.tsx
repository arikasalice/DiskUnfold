import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  ConfirmDialog,
  LiveStatus,
  Notice,
  Progress,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { AnalysisClient } from "../../ipc";
import { readClosingSignal } from "./lifecycle";
import "../../theme/closing.css";

type Closing = { remaining: number };

/**
 * Answers the host's close requests (F15 IF-06 and IF-07).
 *
 * When the window is closed while file changes are running, the host keeps the
 * app open and publishes a confirmation request. This guard asks the user and,
 * only on an explicit confirmation, sends `app.quit` with
 * `confirmUnfinishedChanges`. Once the host reports it is closing, the guard
 * shows progress inline. File changes are never cancelled without that answer.
 */
export function ClosingGuard({
  client,
  children,
}: {
  client: AnalysisClient;
  children: ReactNode;
}) {
  const text = useLocaleText();
  const [changeCount, setChangeCount] = useState<number | null>(null);
  const [closing, setClosing] = useState<Closing | null>(null);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState(false);
  const lastSequence = useRef(0);

  useEffect(
    () =>
      client.subscribe((event) => {
        const signal = readClosingSignal(event);
        if (signal.kind === "ignore" || signal.sequence <= lastSequence.current)
          return;
        lastSequence.current = signal.sequence;
        if (signal.kind === "confirm") {
          setFailed(false);
          setChangeCount(signal.changeCount);
          return;
        }
        setChangeCount(null);
        setClosing({
          remaining: signal.kind === "closing" ? signal.remaining : 0,
        });
      }),
    [client],
  );

  const confirm = useCallback(() => {
    setPending(true);
    setFailed(false);
    client
      .call("app.quit", { mode: "normal", confirmUnfinishedChanges: true })
      .then(() => {
        setChangeCount(null);
        setClosing((current) => current ?? { remaining: changeCount ?? 0 });
      })
      .catch(() => setFailed(true))
      .finally(() => setPending(false));
  }, [client, changeCount]);

  return (
    <>
      {closing !== null && (
        <section
          className="du-closing"
          aria-label={text("ui.closing.progress", "Closing DiskUnfold")}
        >
          <Progress label={text("ui.closing.progress", "Closing DiskUnfold")} />
          <LiveStatus>
            {text(
              "ui.closing.remaining",
              closing.remaining === 1
                ? "Waiting for {count} job to finish."
                : "Waiting for {count} jobs to finish.",
              { count: BigInt(closing.remaining) },
            )}
          </LiveStatus>
        </section>
      )}
      {children}
      {changeCount !== null && closing === null && (
        <ConfirmDialog
          title={text(
            "ui.closing.confirm_title",
            "Close DiskUnfold while file changes are running?",
          )}
          confirmLabel={text(
            "ui.closing.confirm_action",
            "Close and stop changes",
          )}
          cancelLabel={text("ui.closing.keep_open", "Keep DiskUnfold open")}
          isPending={pending}
          onConfirm={confirm}
          onCancel={() => setChangeCount(null)}
        >
          <Text>
            {text(
              "ui.closing.confirm_body",
              changeCount === 1
                ? "{count} file change is still running. Closing now stops it at the next safe point; items it already changed stay changed."
                : "{count} file changes are still running. Closing now stops them at the next safe point; items already changed stay changed.",
              { count: BigInt(changeCount) },
            )}
          </Text>
          {failed && (
            <Notice tone="error">
              {text(
                "ui.closing.failed",
                "DiskUnfold could not start closing. Try again.",
              )}
            </Notice>
          )}
        </ConfirmDialog>
      )}
    </>
  );
}
