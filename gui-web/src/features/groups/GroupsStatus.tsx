import type { ReactNode } from "react";
import {
  Action,
  LiveStatus,
  Notice,
  Progress,
  StatusBadge,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { showsPreviousClassification } from "../../state/groups-controller";
import type { GroupsSnapshot } from "../../state/groups-controller";
import { reasonLabel } from "./group-text";

type Props = {
  snapshot: GroupsSnapshot;
  onClassify: () => void;
  onCancel: () => void;
  /** No rules are saved: the rule editor is the next step, so starting is not emphasized. */
  quietStart?: boolean;
};

function staleMessage(snapshot: GroupsSnapshot, text: LocaleText): string {
  if (snapshot.staleReason === "rules" || snapshot.rulesChanged)
    return text(
      "ui.groups.rules_changed",
      "The rules changed, so the groups must be classified again.",
    );
  if (snapshot.staleReason === "expired")
    return text(
      "ui.groups.requery_expired",
      "This classification is no longer available. Classify again.",
    );
  return text(
    "ui.groups.requery_version",
    "The result changed. Classify again to see the current result.",
  );
}

/** A band: a message with up to two actions, laid out like the common band board. */
function Band({
  tone = "info",
  actions,
  children,
}: {
  tone?: "info" | "error";
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Notice tone={tone}>
      <div className="du-groups-band">
        <Text>{children}</Text>
        {actions && <div className="du-groups-badges">{actions}</div>}
      </div>
    </Notice>
  );
}

function EndState({
  snapshot,
  onClassify,
}: {
  snapshot: GroupsSnapshot;
  onClassify: () => void;
}) {
  const text = useLocaleText();
  const { outcome } = snapshot;
  if (!outcome) return null;
  const failed = outcome.kind === "failed";
  return (
    <Band
      tone={failed ? "error" : "info"}
      actions={
        <Action onPress={onClassify}>
          {failed
            ? text("ui.groups.retry", "Try again")
            : text("ui.groups.reclassify", "Classify again")}
        </Action>
      }
    >
      {failed
        ? `${text("ui.groups.failed", "The classification failed.")} ${reasonLabel(outcome.reasonKey, text)}`
        : text("ui.groups.cancelled", "The classification was cancelled.")}
    </Band>
  );
}

function Banner({
  snapshot,
  onClassify,
  onCancel,
  quietStart = false,
}: Props): ReactNode {
  const text = useLocaleText();
  const again = (
    <Action onPress={onClassify}>
      {text("ui.groups.reclassify", "Classify again")}
    </Action>
  );
  switch (snapshot.phase) {
    case "idle":
      return (
        <Action
          {...(quietStart ? {} : { variant: "primary" as const })}
          onPress={onClassify}
        >
          {text("ui.groups.start", "Start classifying")}
        </Action>
      );
    case "requesting":
      return (
        <>
          <Progress
            label={text("ui.groups.requesting", "Requesting classification…")}
          />
          <LiveStatus>
            {text("ui.groups.requesting", "Requesting classification…")}
          </LiveStatus>
        </>
      );
    case "working":
      return (
        <>
          <Progress label={text("ui.groups.working", "Classifying…")} />
          <LiveStatus>{text("ui.groups.working", "Classifying…")}</LiveStatus>
          <Action onPress={onCancel}>
            {text("ui.groups.cancel", "Cancel classification")}
          </Action>
        </>
      );
    case "showing":
      return <EndState snapshot={snapshot} onClassify={onClassify} />;
    case "updatable":
      return (
        <Band actions={again}>
          {text("ui.groups.updatable", "A newer result is available.")}
        </Band>
      );
    case "requery_needed":
      return <Band actions={again}>{staleMessage(snapshot, text)}</Band>;
    case "not_applicable":
      return (
        <Band>
          {`${text("ui.groups.not_applicable", "This result cannot be grouped.")} ${reasonLabel(snapshot.reasonKey, text)}`}
        </Band>
      );
  }
}

/** Phase banner of the view: progress, cancel, update, retry and inapplicable notices. */
export function GroupsStatus(props: Props) {
  const text = useLocaleText();
  const { snapshot } = props;
  const previous = showsPreviousClassification(snapshot);
  const label =
    snapshot.phase === "requery_needed"
      ? text("ui.groups.previous_version", "Earlier version")
      : text("ui.groups.previous_classification", "Previous classification");
  return (
    <section
      className="du-groups-status"
      aria-label={text("ui.groups.status_label", "Classification status")}
    >
      {previous && <StatusBadge tone="info">{label}</StatusBadge>}
      <Banner {...props} />
    </section>
  );
}
