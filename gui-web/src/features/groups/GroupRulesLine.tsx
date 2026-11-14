import { useSyncExternalStore } from "react";
import { Action, Notice, Progress, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import type { GroupRuleset } from "../../ipc";
import { countConditions } from "../../state/groups-conditions";
import type { GroupsSession } from "./use-groups-session";

/** Names of the stored groups, joined the way the design draws the rule line. */
function ruleNames(ruleset: GroupRuleset): string {
  return ruleset.rules.map((rule) => rule.displayName).join(" + ");
}

/** The "Edit rules" action; it opens the rule editor on the selected group. */
function EditRulesAction({
  session,
  groupId,
  emphasized,
}: {
  session: GroupsSession;
  groupId: string | undefined;
  emphasized: boolean;
}) {
  const text = useLocaleText();
  const snapshot = useSyncExternalStore(
    session.controller.subscribe,
    session.controller.getSnapshot,
  );
  const { ruleset } = snapshot;
  return (
    <div className="du-groups-edit-rules">
      <Action
        {...(emphasized ? { variant: "primary" as const } : {})}
        isDisabled={!ruleset}
        onPress={() => ruleset && session.editor.open(ruleset, groupId)}
      >
        {text("ui.groups.edit_rules", "Edit rules")}
      </Action>
    </div>
  );
}

/**
 * The rule line: the label, the stored rule names, how many conditions they
 * hold and when they were applied, and the action that opens the rule editor.
 */
export function GroupRulesLine({
  session,
  groupId,
  emphasizeEdit,
}: {
  session: GroupsSession;
  groupId: string | undefined;
  /** True when no rules are saved yet, so "Edit rules" is the next step. */
  emphasizeEdit: boolean;
}) {
  const text = useLocaleText();
  const timestamp = useTimestampText();
  const snapshot = useSyncExternalStore(
    session.controller.subscribe,
    session.controller.getSnapshot,
  );
  const { ruleset, rulesStatus } = snapshot;
  const none = ruleset !== undefined && ruleset.rules.length === 0;
  const count = ruleset
    ? ruleset.rules.reduce(
        (sum, rule) => sum + countConditions(rule.condition),
        0,
      )
    : 0;
  const appliedAt = ruleset?.updatedAt;
  return (
    <section
      className="du-groups-rules-line"
      aria-label={text("ui.groups.rules_label", "Rules")}
    >
      <Text kind="muted">{text("ui.groups.rules_label", "Rules")}</Text>
      {ruleset && (
        <div className="du-groups-rules-line__names">
          <div className="du-groups-name">
            <Text kind="path">
              {none
                ? text("ui.groups.rules_none", "No rules are saved yet.")
                : ruleNames(ruleset)}
            </Text>
          </div>
          {!none && (
            <Text kind="muted">
              {appliedAt === undefined
                ? text("ui.groups.conditions_count", "{count} conditions", {
                    count: String(count),
                  })
                : text(
                    "ui.groups.rules_meta",
                    "{count} conditions \u00b7 applied {time}",
                    {
                      count: String(count),
                      time: timestamp(Date.parse(appliedAt), () => appliedAt),
                    },
                  )}
            </Text>
          )}
        </div>
      )}
      {rulesStatus === "loading" && (
        <Progress
          label={text("ui.groups.rules_loading", "Loading the rules…")}
        />
      )}
      {rulesStatus === "error" && (
        <Notice tone="error">
          {text("ui.groups.rules_failed", "Could not read the rules.")}{" "}
          <Action onPress={() => void session.controller.refreshRules()}>
            {text("ui.groups.retry", "Try again")}
          </Action>
        </Notice>
      )}
      <EditRulesAction
        session={session}
        groupId={groupId}
        emphasized={emphasizeEdit}
      />
    </section>
  );
}
