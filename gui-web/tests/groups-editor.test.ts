import { expect, test, vi } from "vitest";
import { AnalysisError } from "../src/ipc";
import { GroupRuleEditor } from "../src/state/groups-editor";
import type { GroupRuleset } from "../src/ipc";
import {
  ARCHIVE,
  MOVIES,
  deferred,
  fakeClient,
  flush,
  ruleset,
} from "./groups-fixtures";
import type { Body } from "./groups-fixtures";

function setup(
  handlers: Record<string, (body: Body) => unknown> = {},
  stored: GroupRuleset = ruleset(),
) {
  const fake = fakeClient({
    "group.rules.get": () =>
      ruleset({ revision: 9, rulesetVersion: 4, fingerprint: "cc" }),
    "group.rules.save": (body: Body) => ({
      outcome: "saved",
      ruleset: ruleset({
        revision: body.expectedRevision + 1,
        rulesetVersion: 4,
        fingerprint: "dd",
      }),
    }),
    ...handlers,
  });
  const onRuleset = vi.fn();
  const editor = new GroupRuleEditor(fake.client, "iec", onRuleset);
  editor.open(stored);
  return {
    fake,
    editor,
    onRuleset,
    keys: () => editor.getSnapshot().drafts.map((rule) => rule.key),
  };
}

test("F36 FR3: cancelling before a save sends nothing and drops the draft", () => {
  const { fake, editor } = setup();
  expect(editor.getSnapshot().phase).toBe("editing");
  editor.requestClose();
  expect(editor.getSnapshot().phase).toBe("viewing");

  const edited = setup();
  edited.editor.rename(edited.keys()[0]!, "Films");
  edited.editor.requestClose();
  // A change would be lost, so the user is asked first; still nothing is sent.
  expect(edited.editor.getSnapshot().confirmingDiscard).toBe(true);
  expect(edited.editor.getSnapshot().phase).toBe("editing");
  edited.editor.keepEditing();
  expect(edited.editor.getSnapshot().drafts[0]?.displayName).toBe("Films");
  edited.editor.requestClose();
  edited.editor.discard();
  expect(edited.editor.getSnapshot().phase).toBe("viewing");
  expect(edited.editor.getSnapshot().drafts).toEqual([]);
  expect(fake.calls).toEqual([]);
  expect(edited.fake.calls).toEqual([]);
});

test("F36 FR3: confirming saves the whole list against the stored revision", async () => {
  const { fake, editor, onRuleset, keys } = setup();
  editor.rename(keys()[0]!, "Films");
  await editor.save();
  expect(fake.calls).toHaveLength(1);
  expect(fake.calls[0]).toMatchObject({ name: "group.rules.save" });
  const body = fake.calls[0]!.body;
  expect(body.expectedRevision).toBe(7);
  expect(body.rules).toHaveLength(2);
  expect(body.rules[0]).toMatchObject({
    groupId: MOVIES,
    displayName: "Films",
  });
  expect(body.rules[1]).toMatchObject({
    groupId: ARCHIVE,
    displayName: "Archive",
  });
  expect(onRuleset).toHaveBeenCalledOnce();
  expect(editor.getSnapshot().phase).toBe("viewing");
});

test("F36 FR3: removing a group saves the list without that rule", async () => {
  const { fake, editor, keys } = setup();
  editor.removeRule(keys()[1]!);
  await editor.save();
  const rules = fake.calls[0]!.body.rules;
  expect(rules).toHaveLength(1);
  expect(rules[0].groupId).toBe(MOVIES);
});

test("F36 FR3: a new group is sent without an identity and with an encoded name condition", async () => {
  const { fake, editor } = setup();
  editor.addRule();
  const key = editor.getSnapshot().selected!;
  expect(editor.getSnapshot().blocker).toBe("empty_name");
  editor.rename(key, "New");
  expect(editor.getSnapshot().blocker).toBe("invalid_input");
  editor.setRow(key, 0, { kind: "name", op: "starts_with", text: "mp4" });
  expect(editor.getSnapshot().blocker).toBeUndefined();
  await editor.save();
  const created = fake.calls[0]!.body.rules[2];
  expect(created).toEqual({
    displayName: "New",
    condition: {
      kind: "predicate",
      predicate: {
        kind: "name",
        op: "starts_with",
        encoding: "windows_utf16le",
        value: "bQBwADQA",
      },
    },
  });
});

test("F36 FR3: a rejected save shows the issue on its rule and keeps the draft", async () => {
  const { editor, onRuleset, keys } = setup({
    "group.rules.save": () => ({
      outcome: "rejected",
      reasonKey: "group.rule_invalid",
      issues: [{ index: 1, reasonKey: "group.name_duplicate" }],
    }),
  });
  editor.rename(keys()[0]!, "Films");
  await editor.save();
  const snapshot = editor.getSnapshot();
  expect(snapshot.phase).toBe("editing");
  expect(snapshot.issues.get(keys()[1]!)).toBe("group.name_duplicate");
  expect(snapshot.issues.has(keys()[0]!)).toBe(false);
  expect(snapshot.selected).toBe(keys()[1]);
  expect(snapshot.drafts[0]?.displayName).toBe("Films");
  expect(onRuleset).not.toHaveBeenCalled();
  // Editing the rejected rule clears its issue.
  editor.rename(keys()[1]!, "Old files");
  expect(editor.getSnapshot().issues.size).toBe(0);
});

test("F36 FR3: a conflict keeps the draft; keeping it rebases the next save on the latest revision", async () => {
  let conflict = true;
  const { fake, editor, keys } = setup({
    "group.rules.save": (body: Body) => {
      if (conflict)
        throw new AnalysisError("STALE_VERSION", "group.rules_modified");
      return {
        outcome: "saved",
        ruleset: ruleset({ revision: body.expectedRevision + 1 }),
      };
    },
  });
  editor.rename(keys()[0]!, "Films");
  await editor.save();
  expect(editor.getSnapshot().phase).toBe("conflict");
  expect(editor.getSnapshot().drafts[0]?.displayName).toBe("Films");
  expect(editor.getSnapshot().blocker).toBeUndefined();
  // Saving is not offered while the conflict is open.
  await editor.save();
  expect(fake.count("group.rules.save")).toBe(1);
  await editor.keepMine();
  expect(editor.getSnapshot().phase).toBe("editing");
  expect(editor.getSnapshot().drafts[0]?.displayName).toBe("Films");
  conflict = false;
  await editor.save();
  expect(fake.calls.at(-1)?.body.expectedRevision).toBe(9);
});

test("F36 FR3: loading the latest rules after a conflict replaces the draft", async () => {
  const { editor, onRuleset, keys } = setup({
    "group.rules.save": () => {
      throw new AnalysisError("STALE_VERSION", "group.rules_modified");
    },
  });
  editor.rename(keys()[0]!, "Films");
  await editor.save();
  await editor.reloadLatest();
  expect(editor.getSnapshot().phase).toBe("editing");
  expect(editor.getSnapshot().drafts[0]?.displayName).toBe("Movies");
  expect(editor.getSnapshot().dirty).toBe(false);
  expect(editor.getSnapshot().base?.revision).toBe(9);
  expect(onRuleset).toHaveBeenCalledOnce();
});

test.each([
  ["BUSY", "db.busy"],
  ["INTERNAL", "support.internal_error"],
] as const)(
  "F36 FR3: %s fails the save without losing the draft and allows a retry",
  async (code, reason) => {
    let fail = true;
    const { fake, editor, keys } = setup({
      "group.rules.save": (body: Body) => {
        if (fail) throw new AnalysisError(code, reason, code === "BUSY");
        return {
          outcome: "saved",
          ruleset: ruleset({ revision: body.expectedRevision + 1 }),
        };
      },
    });
    editor.rename(keys()[0]!, "Films");
    await editor.save();
    expect(editor.getSnapshot().phase).toBe("save_failed");
    expect(editor.getSnapshot().failure?.reasonKey).toBe(reason);
    expect(editor.getSnapshot().drafts[0]?.displayName).toBe("Films");
    fail = false;
    await editor.save();
    expect(fake.count("group.rules.save")).toBe(2);
    expect(editor.getSnapshot().phase).toBe("viewing");
  },
);

test("F36 FR3: while saving, a second confirm sends nothing and input is locked", async () => {
  const gate = deferred<unknown>();
  const { fake, editor, keys } = setup({
    "group.rules.save": () => gate.promise,
  });
  editor.rename(keys()[0]!, "Films");
  const first = editor.save();
  expect(editor.getSnapshot().phase).toBe("saving");
  await editor.save();
  editor.rename(keys()[0]!, "Other");
  editor.addRule();
  editor.requestClose();
  expect(fake.count("group.rules.save")).toBe(1);
  expect(editor.getSnapshot().drafts[0]?.displayName).toBe("Films");
  expect(editor.getSnapshot().drafts).toHaveLength(2);
  expect(editor.getSnapshot().phase).toBe("saving");
  gate.resolve({ outcome: "saved", ruleset: ruleset({ revision: 8 }) });
  await first;
  await flush();
  expect(editor.getSnapshot().phase).toBe("viewing");
});

test("F36 FR3: a save is blocked, with its reason, for an empty name, an empty condition or no change", async () => {
  const { fake, editor, keys } = setup();
  expect(editor.getSnapshot().blocker).toBe("unchanged");
  editor.rename(keys()[0]!, " ");
  expect(editor.getSnapshot().blocker).toBe("empty_name");
  editor.rename(keys()[0]!, "Movies");
  expect(editor.getSnapshot().blocker).toBe("unchanged");
  editor.removeRow(keys()[0]!, 0);
  expect(editor.getSnapshot().blocker).toBe("empty_condition");
  await editor.save();
  expect(fake.calls).toEqual([]);
});

test("F36 FR3: a read-only rule goes back to the host exactly as stored", async () => {
  const stored = ruleset();
  const registered = {
    kind: "predicate" as const,
    predicate: {
      kind: "registered" as const,
      id: "role.video",
      revision: 2,
      operand: "AQ",
    },
  };
  stored.rules[1] = {
    ...stored.rules[1]!,
    condition: registered,
    review: "predicate_unregistered",
  };
  const { fake, editor, keys } = setup({}, stored);
  editor.rename(keys()[0]!, "Films");
  // A read-only rule ignores condition edits.
  editor.addRow(keys()[1]!, "type");
  editor.removeRow(keys()[1]!, 0);
  await editor.save();
  expect(fake.calls[0]!.body.rules[1]).toEqual({
    groupId: ARCHIVE,
    displayName: "Archive",
    condition: registered,
  });
});

test("F36 FR3: rules can be reordered", async () => {
  const { fake, editor, keys } = setup();
  editor.moveRule(keys()[1]!, -1);
  await editor.save();
  expect(
    fake.calls[0]!.body.rules.map((rule: { groupId: string }) => rule.groupId),
  ).toEqual([ARCHIVE, MOVIES]);
});
