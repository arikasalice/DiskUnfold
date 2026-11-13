import { useEffect, useRef } from "react";
import {
  Action,
  Heading,
  LiveStatus,
  Notice,
  Progress,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { ClosingPhase } from "./lifecycle";
import "../../theme/closing.css";

/** Why the closing screen shows a notice besides the progress. */
export type ClosingNotice = "failed" | "already_closing";

/**
 * The closing progress screen (F15 FR7.HD1.gui-web.DD1).
 *
 * It shows the host's lifecycle phase and the remaining job count, and offers
 * "quit without waiting" until the host reports it has closed. It has no
 * cancel action because accepted shutdown is not undone. Focus moves to the
 * screen when it appears and again once the user has chosen to leave, so the
 * disabled button never leaves focus on nothing. The remaining count is
 * announced through one polite live region.
 */
export function ClosingScreen({
  phase,
  remaining,
  leaving,
  pending,
  notice,
  onQuitWithoutWaiting,
}: {
  phase: ClosingPhase;
  remaining: number;
  leaving: boolean;
  pending: boolean;
  notice: ClosingNotice | null;
  onQuitWithoutWaiting: () => void;
}) {
  const text = useLocaleText();
  const screen = useRef<HTMLElement>(null);
  useEffect(() => {
    screen.current?.focus();
  }, [leaving]);

  const count = { count: BigInt(remaining) };
  const remainingText = text(
    "ui.closing.remaining",
    remaining === 1
      ? "Waiting for {count} job to finish."
      : "Waiting for {count} jobs to finish.",
    count,
  );
  return (
    <section
      ref={screen}
      className="du-closing"
      aria-labelledby="du-closing-title"
      tabIndex={-1}
    >
      <Heading level={1} id="du-closing-title">
        {text("ui.closing.progress", "Closing DiskUnfold")}
      </Heading>
      {phase !== "closed" && (
        <Progress label={text("ui.closing.progress", "Closing DiskUnfold")} />
      )}
      <LiveStatus>
        {phase === "closed" ? (
          text("ui.closing.phase_closed", "DiskUnfold has finished closing.")
        ) : (
          <>
            {phase === "access_closed" && (
              <>
                {text(
                  "ui.closing.phase_access_closed",
                  "Stopping new requests and asking running jobs to stop.",
                )}{" "}
              </>
            )}
            {remainingText}
          </>
        )}
      </LiveStatus>
      {leaving && (
        <Notice>
          {text(
            "ui.closing.leaving",
            "Quitting without waiting. Jobs that have not finished are left unfinished and are not recorded as complete.",
          )}
        </Notice>
      )}
      {notice === "already_closing" && (
        <Notice>
          {text("ui.closing.already_closing", "DiskUnfold is already closing.")}
        </Notice>
      )}
      {notice === "failed" && (
        <Notice tone="error">
          {text(
            "ui.closing.failed",
            "DiskUnfold could not start closing. Try again.",
          )}
        </Notice>
      )}
      {phase !== "closed" && (
        <div className="du-closing__actions">
          <Action
            variant="primary"
            isDisabled={leaving || pending}
            onPress={onQuitWithoutWaiting}
          >
            {text("ui.closing.leave_now", "Quit without waiting")}
          </Action>
          <Text kind="muted">
            {text(
              "ui.closing.hint",
              "Waiting lets running jobs finish and record their results.",
            )}
          </Text>
        </div>
      )}
    </section>
  );
}
