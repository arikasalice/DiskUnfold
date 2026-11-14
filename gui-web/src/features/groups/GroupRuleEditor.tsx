import { useSyncExternalStore } from "react";
import type { Ref } from "react";
import {
  Action,
  Choice,
  ChoiceGroup,
  ConfirmDialog,
  Heading,
  Notice,
  Readout,
  ReviewField,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import type {
  GroupRuleEditor as Editor,
  EditorSnapshot,
  SaveBlocker,
} from "../../state/groups-editor";
import type { DraftRule, SizeSystem } from "../../state/groups-conditions";
import { GroupConditionFields } from "./GroupConditionFields";
import { reasonLabel } from "./group-text";

function blockerText(blocker: SaveBlocker, text: LocaleText): string {
  switch (blocker) {
    case "unchanged":
      return text(
        "ui.groups.blocked_unchanged",
        "There are no changes to save.",
      );
    case "empty_name":
      return text("ui.groups.blocked_empty_name", "A group has no name.");
    case "empty_condition":
      return text(
        "ui.groups.blocked_empty_condition",
        "A group has no condition.",
      );
    case "invalid_input":
      return text(
        "ui.groups.blocked_invalid",
        "Check the inputs marked above.",
      );
    case "conflict":
      return text(
        "ui.groups.blocked_conflict",
        "Load the latest rules or keep your changes before saving.",
      );
  }
}

function ruleStatus(
  rule: DraftRule,
  rejected: boolean,
  text: LocaleText,
): string {
  if (rejected) return text("ui.groups.rule_needs_review", "Needs review");
  if (rule.readOnly === "unregistered")
    return text("ui.groups.readonly_unregistered_badge", "Cannot be decided");
  if (rule.readOnly)
    return text("ui.groups.readonly_unsupported_badge", "Not editable here");
  return text("ui.groups.conditions_count", "{count} conditions", {
    count: String(rule.rows.length),
  });
}

function EditorNotices({ state }: { state: EditorSnapshot }) {
  const text = useLocaleText();
  const reason = state.failure
    ? reasonLabel(state.failure.reasonKey, text)
    : "";
  if (state.phase === "conflict")
    return (
      <Notice tone="error">
        {text(
          "ui.groups.conflict_notice",
          "The rules were changed elsewhere. Your edits are kept.",
        )}
      </Notice>
    );
  if (state.phase === "save_failed")
    return (
      <Notice tone="error">
        {text(
          "ui.groups.save_failed",
          "The rules could not be saved. Your edits are kept.",
        )}{" "}
        {reason}
      </Notice>
    );
  if (state.issues.size > 0)
    return (
      <Notice tone="error">
        {text(
          "ui.groups.save_rejected",
          "Some rules were not accepted. Your edits are kept.",
        )}
      </Notice>
    );
  return null;
}

function RuleDetails({
  rule,
  state,
  editor,
  system,
  index,
}: {
  rule: DraftRule;
  state: EditorSnapshot;
  editor: Editor;
  system: SizeSystem;
  index: number;
}) {
  const text = useLocaleText();
  const saving = state.phase === "saving";
  const issue = state.issues.get(rule.key);
  const unnamed = text("ui.groups.unnamed_rule", "(unnamed group)");
  const label = rule.displayName || unnamed;
  return (
    <section
      className="du-groups-rule"
      aria-label={text("ui.groups.rule_details", "Group {name}", {
        name: label,
      })}
    >
      <ReviewField
        label={text("ui.groups.rule_name", "Group name")}
        value={rule.displayName}
        isDisabled={saving}
        description={
          issue
            ? reasonLabel(issue, text)
            : rule.displayName.trim() === ""
              ? text("ui.groups.problem_empty_name", "Enter a group name.")
              : text(
                  "ui.groups.rule_name_hint",
                  "Shown as the name of the virtual group.",
                )
        }
        onChange={(name) => editor.rename(rule.key, name)}
      />
      <GroupConditionFields
        rule={rule}
        system={system}
        disabled={saving}
        onAdd={(kind) => editor.addRow(rule.key, kind)}
        onChange={(row, next) => editor.setRow(rule.key, row, next)}
        onRemove={(row) => editor.removeRow(rule.key, row)}
      />
      <div className="du-groups-badges">
        <Action
          isDisabled={saving || index === 0}
          aria-label={text("ui.groups.move_up", "Move {name} up", {
            name: label,
          })}
          onPress={() => editor.moveRule(rule.key, -1)}
        >
          {text("ui.groups.move_up_short", "Move up")}
        </Action>
        <Action
          isDisabled={saving || index === state.drafts.length - 1}
          aria-label={text("ui.groups.move_down", "Move {name} down", {
            name: label,
          })}
          onPress={() => editor.moveRule(rule.key, 1)}
        >
          {text("ui.groups.move_down_short", "Move down")}
        </Action>
        <Action
          isDisabled={saving}
          aria-label={text("ui.groups.remove_rule", "Remove {name}", {
            name: label,
          })}
          onPress={() => editor.removeRule(rule.key)}
        >
          {text("ui.groups.remove_rule_short", "Remove group")}
        </Action>
      </div>
    </section>
  );
}

/**
 * The in-screen rule editor. It edits a draft; nothing is sent until the user
 * saves, and a cancelled edit sends nothing at all.
 */
export function GroupRuleEditorArea({
  editor,
  system,
  containerRef,
}: {
  editor: Editor;
  system: SizeSystem;
  containerRef?: Ref<HTMLElement>;
}) {
  const text = useLocaleText();
  const timestamp = useTimestampText();
  const state = useSyncExternalStore(editor.subscribe, editor.getSnapshot);
  if (state.phase === "viewing") return null;
  const saving = state.phase === "saving";
  const blocker: SaveBlocker | undefined =
    state.blocker ?? (state.phase === "conflict" ? "conflict" : undefined);
  const selected = state.drafts.find((rule) => rule.key === state.selected);
  const selectedIndex = state.drafts.findIndex(
    (rule) => rule.key === state.selected,
  );
  const updatedAt = state.base?.updatedAt;
  return (
    <section
      ref={containerRef}
      tabIndex={-1}
      className="du-groups-editor"
      aria-label={text("ui.groups.editor_title", "Edit group rules")}
      aria-busy={saving || undefined}
    >
      <Heading level={2}>
        {text("ui.groups.editor_title", "Edit group rules")}
      </Heading>
      <Readout
        label={text("ui.groups.editor_version", "Stored rules")}
        rows={[
          {
            label: text("ui.groups.rules_version_label", "Rules version"),
            value: String(state.base?.rulesetVersion ?? 0),
          },
          {
            label: text("ui.groups.editor_saved_at", "Last saved"),
            value:
              updatedAt === undefined
                ? text("ui.groups.editor_never_saved", "Not saved yet")
                : timestamp(Date.parse(updatedAt), () => updatedAt),
          },
        ]}
      />
      <EditorNotices state={state} />
      {state.phase === "conflict" && (
        <div className="du-groups-badges">
          <Action onPress={() => void editor.reloadLatest()}>
            {text("ui.groups.conflict_reload", "Load the latest rules")}
          </Action>
          <Action onPress={() => void editor.keepMine()}>
            {text(
              "ui.groups.conflict_keep",
              "Keep my changes and review again",
            )}
          </Action>
        </div>
      )}
      <div inert={saving || undefined} className="du-groups-editor__body">
        <ChoiceGroup
          id="du-groups-rule-list"
          label={text("ui.groups.rule_list", "Groups")}
          value={state.selected ?? null}
          isDisabled={saving}
          onChange={(key) => editor.select(key)}
        >
          {state.drafts.map((rule) => (
            <Choice
              key={rule.key}
              value={rule.key}
              label={
                rule.displayName ||
                text("ui.groups.unnamed_rule", "(unnamed group)")
              }
              description={text("ui.groups.virtual_badge", "Virtual group")}
              icon="folder"
              status={{
                text: ruleStatus(rule, state.issues.has(rule.key), text),
                tone: "neutral",
              }}
            />
          ))}
        </ChoiceGroup>
        <Action isDisabled={saving} onPress={() => editor.addRule()}>
          {text("ui.groups.add_rule", "Add group")}
        </Action>
        {selected && (
          <RuleDetails
            rule={selected}
            state={state}
            editor={editor}
            system={system}
            index={selectedIndex}
          />
        )}
      </div>
      <div className="du-groups-badges">
        <Action
          variant="primary"
          isPending={saving}
          isDisabled={saving || blocker !== undefined}
          onPress={() => void editor.save()}
        >
          {saving
            ? text("ui.groups.saving", "Saving…")
            : text("ui.groups.save", "Save")}
        </Action>
        <Action isDisabled={saving} onPress={() => editor.requestClose()}>
          {text("ui.groups.cancel_edit", "Cancel")}
        </Action>
      </div>
      {blocker !== undefined && !saving && (
        <Text kind="muted">{blockerText(blocker, text)}</Text>
      )}
      {state.confirmingDiscard && (
        <ConfirmDialog
          title={text("ui.groups.discard_title", "Discard your changes?")}
          confirmLabel={text("ui.groups.discard_confirm", "Discard changes")}
          cancelLabel={text("ui.groups.discard_keep", "Keep editing")}
          onConfirm={() => editor.discard()}
          onCancel={() => editor.keepEditing()}
        >
          <Text>
            {text(
              "ui.groups.discard_body",
              "Your edits to the group rules have not been saved.",
            )}
          </Text>
        </ConfirmDialog>
      )}
    </section>
  );
}
