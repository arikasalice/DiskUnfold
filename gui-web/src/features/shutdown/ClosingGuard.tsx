import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ConfirmDialog, Notice, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import { AnalysisError } from "../../ipc";
import type { AnalysisClient, AppQuitMode } from "../../ipc";
import { ClosingScreen } from "./ClosingScreen";
import type { ClosingNotice } from "./ClosingScreen";
import { readClosingSignal } from "./lifecycle";
import type { ClosingPhase } from "./lifecycle";
import "../../theme/closing.css";

type Closing = { phase: ClosingPhase; remaining: number };

/**
 * A confirmation the user has to answer. `close` comes from a window close
 * request the host holds open; `leave` comes from "quit without waiting"
 * answered with `confirm_required`.
 */
type Dialog = { intent: "close" | "leave"; changeCount: number };

/**
 * Answers the host's close requests and shows closing progress (F15 IF-06 and
 * IF-07, FR7.HD1.gui-web.DD1).
 *
 * When the window is closed while file changes are running, the host keeps the
 * app open and publishes a confirmation request. The guard asks the user and,
 * only on an explicit confirmation, sends `app.quit` with
 * `confirmUnfinishedChanges`. Once the host reports it is closing, the guard
 * covers the workspace with the closing screen, which offers "quit without
 * waiting". That request is sent unconfirmed first; a `confirm_required` answer
 * opens a confirmation, and the confirmed request repeats the chosen mode. File
 * changes are never cancelled without that answer.
 */
export function ClosingGuard({
  client,
  children,
}: {
  client: AnalysisClient;
  children: ReactNode;
}) {
  const text = useLocaleText();
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [closing, setClosing] = useState<Closing | null>(null);
  const [leaving, setLeaving] = useState(false);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState<ClosingNotice | null>(null);
  const lastSequence = useRef(0);

  useEffect(
    () =>
      client.subscribe((event) => {
        const signal = readClosingSignal(event);
        if (signal.kind === "ignore" || signal.sequence <= lastSequence.current)
          return;
        lastSequence.current = signal.sequence;
        if (signal.kind === "confirm") {
          setNotice(null);
          setDialog({ intent: "close", changeCount: signal.changeCount });
          return;
        }
        setDialog((current) =>
          current?.intent === "leave" && signal.kind === "closing"
            ? current
            : null,
        );
        setClosing(
          signal.kind === "closing"
            ? { phase: signal.phase, remaining: signal.remaining }
            : { phase: "closed", remaining: 0 },
        );
      }),
    [client],
  );

  const quit = useCallback(
    (mode: AppQuitMode, confirmed: boolean, changeCount: number) => {
      setPending(true);
      setNotice(null);
      client
        .call(
          "app.quit",
          confirmed ? { mode, confirmUnfinishedChanges: true } : { mode },
        )
        .then((response) => {
          if (response.outcome === "confirm_required") {
            setDialog({
              intent: mode === "without_waiting" ? "leave" : "close",
              changeCount: response.inProgressChangeCount ?? changeCount,
            });
          } else if (
            response.outcome === "accepted" ||
            response.outcome === "already_closing"
          ) {
            setDialog(null);
            if (mode === "without_waiting") setLeaving(true);
            setClosing(
              (current) =>
                current ?? { phase: "access_closed", remaining: changeCount },
            );
          } else {
            setNotice("failed");
          }
        })
        .catch((error: unknown) => {
          // A refusal after acceptance cannot be retried, so do not suggest it.
          if (
            error instanceof AnalysisError &&
            error.reasonKey === "startup.shutting_down"
          ) {
            setDialog(null);
            setNotice("already_closing");
          } else {
            setNotice("failed");
          }
        })
        .finally(() => setPending(false));
    },
    [client],
  );

  const confirm = useCallback(() => {
    if (dialog === null) return;
    quit(
      dialog.intent === "leave" ? "without_waiting" : "normal",
      true,
      dialog.changeCount,
    );
  }, [dialog, quit]);

  const showDialog =
    dialog !== null && (dialog.intent === "leave" || closing === null);

  return (
    <>
      {closing !== null && (
        <ClosingScreen
          phase={closing.phase}
          remaining={closing.remaining}
          leaving={leaving}
          pending={pending}
          notice={showDialog ? null : notice}
          onQuitWithoutWaiting={() => quit("without_waiting", false, 0)}
        />
      )}
      <div
        className="du-closing-host"
        inert={closing !== null}
        aria-hidden={closing !== null ? true : undefined}
      >
        {children}
      </div>
      {showDialog && (
        <ConfirmDialog
          title={
            dialog.intent === "leave"
              ? text(
                  "ui.closing.leave_title",
                  "Quit without waiting while file changes are running?",
                )
              : text(
                  "ui.closing.confirm_title",
                  "Close DiskUnfold while file changes are running?",
                )
          }
          confirmLabel={
            dialog.intent === "leave"
              ? text("ui.closing.leave_now", "Quit without waiting")
              : text("ui.closing.confirm_action", "Close and stop changes")
          }
          cancelLabel={
            dialog.intent === "leave"
              ? text("ui.closing.keep_waiting", "Keep waiting")
              : text("ui.closing.keep_open", "Keep DiskUnfold open")
          }
          isPending={pending}
          onConfirm={confirm}
          onCancel={() => setDialog(null)}
        >
          <Text>
            {text(
              "ui.closing.confirm_body",
              dialog.changeCount === 1
                ? "{count} file change is still running. If you close now, the item in progress is finished and the remaining items are not processed."
                : "{count} file changes are still running. If you close now, the item in progress is finished and the remaining items are not processed.",
              { count: BigInt(dialog.changeCount) },
            )}
          </Text>
          {notice === "failed" && (
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
