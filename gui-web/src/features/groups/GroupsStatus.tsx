import type { ReactNode } from "react";
import {
  Action,
  LiveStatus,
  Notice,
  Progress,
  StatusBadge,
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
    <Notice tone={failed ? "error" : "info"}>
      {failed
        ? `${text("ui.groups.failed", "The classification failed.")} ${reasonLabel(outcome.reasonKey, text)}`
        : text("ui.groups.cancelled", "The classification was cancelled.")}{" "}
      <Action onPress={onClassify}>
        {failed
          ? text("ui.groups.retry", "Try again")
          : text("ui.groups.reclassify", "Classify again")}
      </Action>
    </Notice>
  );
}

function Banner({ snapshot, onClassify, onCancel }: Props): ReactNode {
  const text = useLocaleText();
  switch (snapshot.phase) {
    case "idle":
      return (
        <Action variant="primary" onPress={onClassify}>
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
        <Notice>
          {text("ui.groups.updatable", "A newer result is available.")}{" "}
          <Action onPress={onClassify}>
            {text("ui.groups.reclassify", "Classify again")}
          </Action>
        </Notice>
      );
    case "requery_needed":
      return (
        <Notice>
          {staleMessage(snapshot, text)}{" "}
          <Action onPress={onClassify}>
            {text("ui.groups.reclassify", "Classify again")}
          </Action>
        </Notice>
      );
    case "not_applicable":
      return (
        <Notice>
          {text("ui.groups.not_applicable", "This result cannot be grouped.")}{" "}
          {reasonLabel(snapshot.reasonKey, text)}
        </Notice>
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
