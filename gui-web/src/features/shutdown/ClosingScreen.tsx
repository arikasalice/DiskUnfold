import { useEffect, useRef, useState } from "react";
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

/** What each lifecycle phase shows; a new phase is one row here. */
const PHASES: Record<
  ClosingPhase,
  { phaseKey: string | null; progress: boolean; action: boolean }
> = {
  access_closed: {
    phaseKey: "ui.closing.phase_access_closed",
    progress: true,
    action: true,
  },
  draining: { phaseKey: null, progress: true, action: true },
  closed: {
    phaseKey: "ui.closing.phase_closed",
    progress: false,
    action: false,
  },
};

const PHASE_FALLBACK: Record<string, string> = {
  "ui.closing.phase_access_closed":
    "Stopping new requests and asking running jobs to stop.",
  "ui.closing.phase_closed": "DiskUnfold has finished closing.",
};

/**
 * The closing progress screen (F15 FR7.HD1.gui-web.DD1).
 *
 * It shows the host's lifecycle phase and the remaining job count, and offers
 * "quit without waiting" until the host reports it has closed. It has no
 * cancel action because accepted shutdown is not undone. The guard stops
 * rendering the workspace when this screen appears, so focus moves here on
 * mount and again once the user has chosen to leave (the button is then
 * disabled and cannot keep focus).
 */
export function ClosingScreen({
  phase,
  remaining,
  leaving,
  notice,
  onQuitWithoutWaiting,
}: {
  phase: ClosingPhase;
  remaining: number;
  leaving: boolean;
  notice: ClosingNotice | null;
  onQuitWithoutWaiting: () => void;
}) {
  const text = useLocaleText();
  const screen = useRef<HTMLElement>(null);
  useEffect(() => {
    screen.current?.focus();
  }, [leaving]);
  const row = PHASES[phase];
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
      {row.progress && (
        <Progress label={text("ui.closing.progress_bar", "Closing progress")} />
      )}
      <ClosingStatus phase={phase} remaining={remaining} />
      <ClosingNotices leaving={leaving} notice={notice} />
      {row.action && (
        <div className="du-closing__actions">
          <Action
            variant="primary"
            isDisabled={leaving}
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

/**
 * The phase sentence and the remaining count. The live region starts empty and
 * is filled after mount, so assistive technology announces the first count.
 */
function ClosingStatus({
  phase,
  remaining,
}: {
  phase: ClosingPhase;
  remaining: number;
}) {
  const text = useLocaleText();
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);
  const { phaseKey, action } = PHASES[phase];
  const phaseText =
    phaseKey === null ? null : text(phaseKey, PHASE_FALLBACK[phaseKey] ?? "");
  const remainingText = text(
    "ui.closing.remaining",
    remaining === 1
      ? "Waiting for {count} job to finish."
      : "Waiting for {count} jobs to finish.",
    { count: BigInt(remaining) },
  );
  // The closed phase has no count; its sentence is the announcement.
  const live = action ? remainingText : phaseText;
  return (
    <>
      {action && phaseText !== null && <Text>{phaseText}</Text>}
      <LiveStatus>{ready ? live : ""}</LiveStatus>
    </>
  );
}

function ClosingNotices({
  leaving,
  notice,
}: {
  leaving: boolean;
  notice: ClosingNotice | null;
}) {
  const text = useLocaleText();
  return (
    <>
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
    </>
  );
}
