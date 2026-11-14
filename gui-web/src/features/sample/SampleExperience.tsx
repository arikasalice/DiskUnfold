import { useState } from "react";
import type { ReactNode } from "react";
import {
  Action,
  ExampleBadge,
  Heading,
  Notice,
  SampleBanner,
  SelectField,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { SampleObservationRef, SampleSession } from "../../ipc";
import type { SampleCommand, SampleProblem } from "../../state/sample-session";
import { useSampleConfirm } from "./SampleConfirm";
import type { SampleConfirmKind } from "./SampleConfirm";
import {
  SampleProblemNotice,
  SampleProgress,
  SampleUnavailable,
} from "./SampleStates";
import "../../theme/sample.css";

export type SampleExperienceProps = {
  session: SampleSession | undefined;
  pending: SampleCommand | undefined;
  problem: SampleProblem | undefined;
  onReset: () => Promise<unknown>;
  /** Resolves true once the session no longer exists. */
  onClose: () => Promise<boolean>;
  /** Called after the session was released, to start a fresh real analysis. */
  onRealAnalysis: () => void;
  onDismiss: () => void;
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

function SampleStrip({
  session,
  busy,
  ask,
}: {
  session: SampleSession;
  busy: boolean;
  ask: Ask;
}) {
  const text = useLocaleText();
  // An unavailable sample offers only retry and real analysis, in its own notice.
  const actions = session.state !== "unavailable";
  return (
    <SampleBanner
      label={text("ui.sample.banner_label", "Sample experience")}
      status={text("ui.sample.banner_status", "Sample experience in progress")}
      detail={
        session.bundleVersion
          ? text("ui.sample.bundle_version", "Sample set version {version}", {
              version: session.bundleVersion,
            })
          : undefined
      }
    >
      {actions && (
        <>
          <Action
            isDisabled={busy}
            onPress={(event) => ask("end", event.target)}
          >
            {text("ui.sample.end", "End experience")}
          </Action>
          <Action
            variant="primary"
            isDisabled={busy}
            onPress={(event) => ask("real", event.target)}
          >
            {text("ui.sample.real_analysis", "Start real analysis")}
          </Action>
        </>
      )}
    </SampleBanner>
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
  switch (session.state) {
    case "preparing":
    case "resetting":
      return <SampleProgress resetting={session.state === "resetting"} />;
    case "unavailable":
      return (
        <SampleUnavailable
          reason={session.unavailableReason}
          isDisabled={busy}
          onRetry={(trigger) => ask("reset", trigger)}
          onRealAnalysis={(trigger) => ask("real", trigger)}
        />
      );
    case "active":
      return (
        <ActiveSample session={session} busy={busy} ask={ask}>
          {children}
        </ActiveSample>
      );
    default:
      return null;
  }
}

/**
 * Everything the sample experience shows: the global strip, failure notices
 * and the sample result context. The caller keeps the real analysis hidden
 * while a session is open, so no sample state reaches a real result list.
 */
export function SampleExperience(props: SampleExperienceProps) {
  const { session, pending, problem } = props;
  const text = useLocaleText();
  const busy = pending !== undefined;
  const { ask, dialog } = useSampleConfirm(async (kind: SampleConfirmKind) => {
    if (kind === "reset") await props.onReset();
    else if ((await props.onClose()) && kind === "real") props.onRealAnalysis();
  });
  return (
    <>
      {session && <SampleStrip session={session} busy={busy} ask={ask} />}
      <SampleProblemNotice problem={problem} onDismiss={props.onDismiss} />
      {session && (
        <section
          className="du-sample"
          aria-label={text("ui.sample.heading", "Sample data experience")}
        >
          <header className="du-sample__header">
            <Heading level={2}>
              {text("ui.sample.heading", "Sample data experience")}
            </Heading>
            <ExampleBadge>
              {text("ui.sample.example_badge", "Example")}
            </ExampleBadge>
          </header>
          <SampleBody session={session} busy={busy} ask={ask}>
            {props.children}
          </SampleBody>
        </section>
      )}
      {dialog}
    </>
  );
}
