import "./groups-editor.css";
import { useSyncExternalStore } from "react";
import type { Ref } from "react";
import {
  Action,
  ConfirmDialog,
  Heading,
  Notice,
  ReviewField,
  SelectField,
  StatusBadge,
  Text,
} from "../../components";
import type { SizeDisplay } from "../../i18n/bytes";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { GroupSummary, GroupTotals } from "../../ipc";
import type { DraftRule } from "../../state/groups-conditions";
import type { ReadState, TotalsKey } from "../../state/groups-details";
import type {
  GroupRuleEditor as Editor,
  EditorSnapshot,
  SaveBlocker,
} from "../../state/groups-editor";
import type { GroupPreview } from "../../state/groups-preview";
import { GroupConditionFields } from "./GroupConditionFields";
import { GroupPreviewPanel } from "./GroupPreviewPanel";
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
    case "empty_value":
      return text(
        "ui.groups.blocked_empty_value",
        "A condition has no value, so the rules cannot be saved.",
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

function RuleFields({
  rule,
  state,
  editor,
  system,
}: {
  rule: DraftRule;
  state: EditorSnapshot;
  editor: Editor;
  system: SizeDisplay["system"];
}) {
  const text = useLocaleText();
  const saving = state.phase === "saving";
  const issue = state.issues.get(rule.key);
  return (
    <div className="du-groups-rule">
      <ReviewField
        label={text("ui.groups.rule_name", "Group name")}
        value={rule.displayName}
        isDisabled={saving}
        description={
          issue
            ? reasonLabel(issue, text)
            : rule.displayName.trim() === ""
              ? text("ui.groups.problem_empty_name", "Enter a group name.")
              : ""
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
    </div>
  );
}

/**
 * Which group is edited, and the actions that add, remove and reorder groups.
 * The design draws the editor for one group, so these stay in one quiet row.
 */
function GroupList({
  state,
  editor,
}: {
  state: EditorSnapshot;
  editor: Editor;
}) {
  const text = useLocaleText();
  const saving = state.phase === "saving";
  const unnamed = text("ui.groups.unnamed_rule", "(unnamed group)");
  const selected = state.drafts.find((rule) => rule.key === state.selected);
  const index = state.drafts.findIndex((rule) => rule.key === state.selected);
  const label = selected?.displayName || unnamed;
  return (
    <section
      className="du-groups-manage"
      aria-label={text("ui.groups.rule_list", "Groups")}
    >
      {state.drafts.length > 0 && (
        <SelectField<string>
          label={text("ui.groups.rule_picker", "Group being edited")}
          value={state.selected ?? ""}
          options={state.drafts.map((rule) => ({
            value: rule.key,
            label: rule.displayName || unnamed,
          }))}
          onChange={(key) => !saving && editor.select(key)}
        />
      )}
      <div className="du-groups-badges">
        <Action
          variant="quiet"
          isDisabled={saving}
          onPress={() => editor.addRule()}
        >
          {text("ui.groups.add_rule", "Add group")}
        </Action>
        {selected && (
          <>
            <Action
              variant="quiet"
              isDisabled={saving || index === 0}
              aria-label={text("ui.groups.move_up", "Move {name} up", {
                name: label,
              })}
              onPress={() => editor.moveRule(selected.key, -1)}
            >
              {text("ui.groups.move_up_short", "Move up")}
            </Action>
            <Action
              variant="quiet"
              isDisabled={saving || index === state.drafts.length - 1}
              aria-label={text("ui.groups.move_down", "Move {name} down", {
                name: label,
              })}
              onPress={() => editor.moveRule(selected.key, 1)}
            >
              {text("ui.groups.move_down_short", "Move down")}
            </Action>
            <Action
              variant="quiet"
              isDisabled={saving}
              aria-label={text("ui.groups.remove_rule", "Remove {name}", {
                name: label,
              })}
              onPress={() => editor.removeRule(selected.key)}
            >
              {text("ui.groups.remove_rule_short", "Remove group")}
            </Action>
          </>
        )}
      </div>
    </section>
  );
}

/**
 * The in-screen rule editor, drawn to the right of the applied result. It
 * edits a draft; nothing is sent until the user applies, and a cancelled edit
 * sends nothing at all. The preview classifies the draft without storing it.
 */
export function GroupRuleEditorArea({
  editor,
  preview,
  system,
  display,
  summary,
  totals,
  containerRef,
}: {
  editor: Editor;
  preview: GroupPreview;
  system: SizeDisplay["system"];
  display: SizeDisplay;
  summary: GroupSummary | undefined;
  totals: ReadState<TotalsKey, GroupTotals>;
  containerRef?: Ref<HTMLElement>;
}) {
  const text = useLocaleText();
  const state = useSyncExternalStore(editor.subscribe, editor.getSnapshot);
  if (state.phase === "viewing") return null;
  const saving = state.phase === "saving";
  const blocker: SaveBlocker | undefined =
    state.blocker ?? (state.phase === "conflict" ? "conflict" : undefined);
  const selected = state.drafts.find((rule) => rule.key === state.selected);
  return (
    <section
      ref={containerRef}
      tabIndex={-1}
      className="du-groups-editor"
      aria-label={text("ui.groups.editor_title", "Edit group rules")}
      aria-busy={saving || undefined}
    >
      <div className="du-groups-editor__head">
        <Heading level={3}>
          {text("ui.groups.editor_title", "Edit group rules")}
        </Heading>
        <StatusBadge tone="warning">
          {text("ui.groups.not_applied_badge", "Not applied yet")}
        </StatusBadge>
      </div>
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
        {selected ? (
          <RuleFields
            rule={selected}
            state={state}
            editor={editor}
            system={system}
          />
        ) : (
          <Text kind="muted">
            {text(
              "ui.groups.no_rules_in_editor",
              "There are no groups. Add one.",
            )}
          </Text>
        )}
        <GroupPreviewPanel
          preview={preview}
          rule={selected}
          summary={summary}
          totals={totals}
          display={display}
        />
        <GroupList state={state} editor={editor} />
      </div>
      <div className="du-groups-editor__actions">
        <Action isDisabled={saving} onPress={() => editor.requestClose()}>
          {text("ui.groups.cancel_edit", "Cancel")}
        </Action>
        <div className="du-groups-editor__apply">
          {blocker !== undefined && !saving && (
            <Text kind="muted">{blockerText(blocker, text)}</Text>
          )}
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
        </div>
      </div>
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
              "Your edits to the group rules have not been applied.",
            )}
          </Text>
        </ConfirmDialog>
      )}
    </section>
  );
}
