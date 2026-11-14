import { useEffect, useRef } from "react";
import {
  Action,
  DetailDisclosure,
  Heading,
  Notice,
  Progress,
  Readout,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { GroupDecision, GroupExplanation } from "../../ipc";
import type { ExplainKey, ReadState } from "../../state/groups-details";
import { memberTextValue } from "./group-format";
import { reasonLabel } from "./group-text";

function decisionText(decision: GroupDecision, text: LocaleText): string {
  if (decision === "match") return text("ui.groups.decision_match", "Matches");
  if (decision === "no_match")
    return text("ui.groups.decision_no_match", "Does not match");
  return text("ui.groups.decision_undecidable", "Cannot decide");
}

function nodeLabel(path: readonly number[], text: LocaleText): string {
  return path.length === 0
    ? text("ui.groups.node_root", "Whole condition")
    : text("ui.groups.node", "Condition {path}", {
        path: path.map((index) => index + 1).join("."),
      });
}

/** Why one file is, or is not, in each group: a decision per rule and per condition node. */
export function GroupExplainPanel({
  state,
  groupNames,
  onRetry,
  onClose,
}: {
  state: ReadState<ExplainKey, GroupExplanation>;
  groupNames: ReadonlyMap<string, string>;
  onRetry: () => void;
  onClose: () => void;
}) {
  const text = useLocaleText();
  const panel = useRef<HTMLElement>(null);
  // A panel opened by a row takes focus so keyboard users land on the reasons.
  useEffect(() => {
    panel.current?.focus();
  }, []);
  if (state.phase === "idle") return null;
  const name = (id: string) =>
    groupNames.get(id) ?? text("ui.groups.group_missing", "Unknown group");
  return (
    <section
      ref={panel}
      tabIndex={-1}
      className="du-groups-explain"
      aria-label={text("ui.groups.explain_title", "Classification reasons")}
    >
      <div className="du-groups-explain__header">
        <Heading level={3}>
          {text("ui.groups.explain_title", "Classification reasons")}
        </Heading>
        <Action variant="quiet" onPress={onClose}>
          {text("ui.groups.explain_close", "Close reasons")}
        </Action>
      </div>
      {state.phase === "loading" && (
        <Progress
          label={text("ui.groups.explain_loading", "Loading reasons…")}
        />
      )}
      {state.phase === "error" && (
        <Notice tone="error">
          {text("ui.groups.explain_failed", "Could not load the reasons.")}{" "}
          {reasonLabel(state.reasonKey, text)}{" "}
          <Action onPress={onRetry}>
            {text("ui.groups.retry", "Try again")}
          </Action>
        </Notice>
      )}
      {state.phase === "ready" && (
        <>
          <Text kind="path">
            {memberTextValue(state.value.displayPath, text)}
          </Text>
          <Readout
            label={text("ui.groups.explain_rules", "Decision of each rule")}
            rows={state.value.rules.map((rule) => ({
              label: name(rule.groupId),
              value: decisionText(rule.decision, text),
              ...(rule.reasonKey
                ? { notes: [reasonLabel(rule.reasonKey, text)] }
                : {}),
            }))}
          />
          <DetailDisclosure
            title={text(
              "ui.groups.explain_nodes",
              "Decision of each condition",
            )}
          >
            {state.value.rules.map((rule) => (
              <Readout
                key={rule.groupId}
                label={name(rule.groupId)}
                rows={rule.nodes.map((node) => ({
                  label: nodeLabel(node.path, text),
                  value: decisionText(node.decision, text),
                  ...(node.predicate
                    ? {
                        notes: [
                          `${node.predicate.id} (${node.predicate.revision})`,
                        ],
                      }
                    : {}),
                }))}
              />
            ))}
          </DetailDisclosure>
        </>
      )}
    </section>
  );
}
