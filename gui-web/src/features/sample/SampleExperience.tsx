import { useState } from "react";
import type { ReactNode } from "react";
import { Action, Band, Notice, SelectField } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { SampleObservationRef, SampleSession } from "../../ipc";
import type { SampleCommand, SampleProblem } from "../../state/sample-session";
import { useSampleConfirm } from "./SampleConfirm";
import type { SampleConfirmKind } from "./SampleConfirm";
import { SampleAnnouncer } from "./SampleAnnouncer";
import { SampleProblemNotice, SampleProgress } from "./SampleStates";
import "../../theme/sample.css";

export type SampleExperienceProps = {
  /** The held session, whatever its state; the sample screen shows only while `screen` is true. */
  session: SampleSession | undefined;
  /** True once the session has been active and has not failed since. */
  reached: boolean;
  /** True while the sample result screen owns the main area. */
  screen: boolean;
  pending: SampleCommand | undefined;
  problem: SampleProblem | undefined;
  onReset: () => Promise<unknown>;
  /** Resolves true once the session no longer exists. */
  onClose: () => Promise<boolean>;
  /** Called after the session was released, to start a fresh real analysis. */
  onRealAnalysis: () => void;
  onDismiss: () => void;
  /** Gives the focus to the entry button after the sample ended under the dialog. */
  onFocusEntry: () => void;
  /** Renders the result browser for one sample result version. */
  children: (resultVersion: string) => ReactNode;
};

type Ask = (kind: SampleConfirmKind, trigger: Element) => void;
type Choice = { sessionRef: string; index: string };

function observationOptions(
  observations: readonly SampleObservationRef[],
  text: LocaleText,
) {
  return observations.map((item) => ({
    value: String(item.index),
    label: item.observedAt
      ? text("ui.sample.observation_option", "Observation {number} ({time})", {
          number: String(item.index + 1),
          time: item.observedAt.slice(0, 10),
        })
      : text("ui.sample.observation_untimed", "Observation {number}", {
          number: String(item.index + 1),
        }),
  }));
}

/** The observation shown: the user's pick for this session, else the latest. */
function pickObservation(
  session: SampleSession,
  choice: Choice | undefined,
): SampleObservationRef | undefined {
  const wanted =
    choice?.sessionRef === session.sessionRef ? choice.index : undefined;
  return (
    session.observations.find((item) => String(item.index) === wanted) ??
    session.observations.at(-1)
  );
}

function ActiveSample({
  session,
  busy,
  ask,
  children,
}: {
  session: SampleSession;
  busy: boolean;
  ask: Ask;
  children: SampleExperienceProps["children"];
}) {
  const text = useLocaleText();
  const [choice, setChoice] = useState<Choice>();
  const shown = pickObservation(session, choice);
  return (
    <>
      <div className="du-sample__toolbar">
        {session.observations.length > 1 && shown && (
          <SelectField
            label={text("ui.sample.observation", "Observation time")}
            value={String(shown.index)}
            options={observationOptions(session.observations, text)}
            onChange={(index) =>
              setChoice({ sessionRef: session.sessionRef, index })
            }
          />
        )}
        <Action
          icon="refresh"
          isDisabled={busy}
          onPress={(event) => ask("reset", event.target)}
        >
          {text("ui.sample.reset", "Reset experience")}
        </Action>
      </div>
      {shown ? (
        children(shown.resultVersion)
      ) : (
        <Notice>
          {text(
            "ui.sample.no_observation",
            "This sample has no observation to show.",
          )}
        </Notice>
      )}
    </>
  );
}

/** The strip: its statement first, then real analysis, then closing the sample. */
function SampleStrip({ busy, ask }: { busy: boolean; ask: Ask }) {
  const text = useLocaleText();
  return (
    <Band
      label={text("ui.sample.banner_label", "Sample experience")}
      focusable
      actions={[
        <Action
          key="real"
          variant="primary"
          isDisabled={busy}
          onPress={(event) => ask("real", event.target)}
        >
          {text("ui.sample.real_analysis", "Start real analysis")}
        </Action>,
        <Action
          key="close"
          isDisabled={busy}
          onPress={(event) => ask("end", event.target)}
        >
          {text("ui.sample.close", "Close sample")}
        </Action>,
      ]}
    >
      {text(
        "ui.sample.banner_statement",
        "Sample experience · Separate from your files · Changes cannot run",
      )}
    </Band>
  );
}

function SampleBody({
  session,
  busy,
  ask,
  children,
}: {
  session: SampleSession;
  busy: boolean;
  ask: Ask;
  children: SampleExperienceProps["children"];
}) {
  if (session.state === "active")
    return (
      <ActiveSample session={session} busy={busy} ask={ask}>
        {children}
      </ActiveSample>
    );
  // Only an active session that is prepared again reaches the screen otherwise.
  return <SampleProgress resetting />;
}

/**
 * Everything the sample result screen shows: the global strip, failure
 * notices and the result context. The caller keeps the real analysis hidden
 * while the screen is open, so no sample state reaches a real result list.
 * The state announcer stays mounted throughout, also before and after the
 * screen, so each state change is spoken once.
 */
export function SampleExperience(props: SampleExperienceProps) {
  const { session, pending, problem, screen } = props;
  const text = useLocaleText();
  const busy = pending !== undefined;
  const { ask, dialog } = useSampleConfirm(async (kind: SampleConfirmKind) => {
    if (kind === "reset") await props.onReset();
    else if ((await props.onClose()) && kind === "real") props.onRealAnalysis();
  }, props.onFocusEntry);
  return (
    <>
      <SampleAnnouncer session={session} reached={props.reached} />
      {screen && session && (
        <>
          <SampleStrip busy={busy} ask={ask} />
          <SampleProblemNotice problem={problem} onDismiss={props.onDismiss} />
          <section
            className="du-sample"
            aria-label={text("ui.sample.heading", "Sample data experience")}
          >
            <SampleBody session={session} busy={busy} ask={ask}>
              {props.children}
            </SampleBody>
          </section>
        </>
      )}
      {dialog}
    </>
  );
}
