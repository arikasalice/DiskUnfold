import "./browse-scroll-fixture";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { AnalysisError } from "../src/ipc";
import {
  ARCHIVE,
  MOVIES,
  deferred,
  flush,
  ruleset,
  summary,
  total,
  totals,
} from "./groups-fixtures";
import type { Body } from "./groups-fixtures";
import { classified, mount } from "./groups-view-helpers";

test("F36 FR3: the editor cancels without a request; Escape closes only the editor and asks before dropping changes", async () => {
  const { fake } = mount();
  await screen.findByText("Movies + Archive");
  const edit = screen.getByRole("button", { name: "Edit rules" });
  fireEvent.click(edit);
  const editor = await screen.findByRole("region", {
    name: "Edit group rules",
  });
  expect(
    screen.getByRole("button", { name: "Save" }).hasAttribute("disabled"),
  ).toBe(true);
  expect(
    within(editor).getByText("There are no changes to save."),
  ).toBeDefined();
  fireEvent.keyDown(editor, { key: "Escape" });
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Edit group rules" }),
    ).toBeNull(),
  );
  // The screen itself stays.
  expect(
    screen.getByRole("heading", { name: "Group by my rules" }),
  ).toBeDefined();

  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const name = await screen.findByRole("textbox", { name: "Group name" });
  fireEvent.change(name, { target: { value: "Films" } });
  fireEvent.keyDown(name, { key: "Escape" });
  await screen.findByRole("alertdialog");
  expect(screen.getByText("Discard your changes?")).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(
    (screen.getByRole("textbox", { name: "Group name" }) as HTMLInputElement)
      .value,
  ).toBe("Films");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await screen.findByRole("alertdialog");
  fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Edit group rules" }),
    ).toBeNull(),
  );
  expect(fake.count("group.rules.save")).toBe(0);
});

test("F36 FR3: the save button is blocked while saving and a second press sends nothing; composing text sends nothing", async () => {
  const gate = deferred<unknown>();
  const { fake } = mount({ "group.rules.save": () => gate.promise });
  await screen.findByText("Movies + Archive");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const name = await screen.findByRole("textbox", { name: "Group name" });
  fireEvent.compositionStart(name);
  fireEvent.change(name, { target: { value: "Fil" } });
  fireEvent.keyDown(name, { key: "Enter" });
  expect(fake.count("group.rules.save")).toBe(0);
  fireEvent.compositionEnd(name);
  fireEvent.change(name, { target: { value: "Films" } });
  const save = screen.getByRole("button", { name: "Save" });
  fireEvent.click(save);
  await screen.findByRole("button", { name: "Saving…" });
  fireEvent.click(screen.getByRole("button", { name: "Saving…" }));
  fireEvent.click(screen.getByRole("button", { name: "Saving…" }));
  expect(fake.count("group.rules.save")).toBe(1);
  expect(
    screen
      .getByRole("region", { name: "Edit group rules" })
      .getAttribute("aria-busy"),
  ).toBe("true");
  gate.resolve({
    outcome: "saved",
    ruleset: ruleset({ revision: 8, rulesetVersion: 4, fingerprint: "zz" }),
  });
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Edit group rules" }),
    ).toBeNull(),
  );
});

test("F36 FR3: a rejected save shows the reason on the rule; a conflict keeps the input and offers the latest rules", async () => {
  let mode: "rejected" | "conflict" = "rejected";
  mount({
    "group.rules.save": () => {
      if (mode === "conflict")
        throw new AnalysisError("STALE_VERSION", "group.rules_modified");
      return {
        outcome: "rejected",
        reasonKey: "group.rule_invalid",
        issues: [{ index: 0, reasonKey: "group.name_duplicate" }],
      };
    },
  });
  await screen.findByText("Movies + Archive");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const name = await screen.findByRole("textbox", { name: "Group name" });
  fireEvent.change(name, { target: { value: "Archive" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Two groups have the same name.");
  expect(
    screen.getByText("Some rules were not accepted. Your edits are kept."),
  ).toBeDefined();
  expect(
    (screen.getByRole("textbox", { name: "Group name" }) as HTMLInputElement)
      .value,
  ).toBe("Archive");

  mode = "conflict";
  fireEvent.change(screen.getByRole("textbox", { name: "Group name" }), {
    target: { value: "Films" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText(
    "The rules were changed elsewhere. Your edits are kept.",
  );
  expect(
    screen.getByRole("button", { name: "Load the latest rules" }),
  ).toBeDefined();
  expect(
    screen.getByRole("button", { name: "Keep my changes and review again" }),
  ).toBeDefined();
  expect(
    screen.getByRole("button", { name: "Save" }).hasAttribute("disabled"),
  ).toBe(true);
  expect(
    (screen.getByRole("textbox", { name: "Group name" }) as HTMLInputElement)
      .value,
  ).toBe("Films");
});

test("F36 FR3: a rule with an unregistered condition shows as not decidable and cannot be edited", async () => {
  const stored = ruleset();
  stored.rules[1] = {
    ...stored.rules[1]!,
    condition: {
      kind: "predicate",
      predicate: {
        kind: "registered",
        id: "role.video",
        revision: 1,
        operand: "AQ",
      },
    },
    review: "predicate_unregistered",
  };
  mount({ "group.rules.get": () => stored });
  await screen.findByText("Movies + Archive");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  await screen.findByRole("region", { name: "Edit group rules" });
  const picker = screen.getByRole("combobox", { name: "Group being edited" });
  const archive = within(picker).getByRole("option", { name: "Archive" });
  fireEvent.change(picker, {
    target: { value: archive.getAttribute("value") },
  });
  expect(
    (await screen.findAllByText("Cannot be decided")).length,
  ).toBeGreaterThan(0);
  expect(screen.getByText(/kept as it is and cannot be edited/)).toBeDefined();
  expect(
    screen.queryByRole("button", { name: "Add name condition" }),
  ).toBeNull();
});

test("F36 FR3: after a save the view says the groups must be classified again and does not classify", async () => {
  const { fake } = await classified();
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "Group name" }), {
    target: { value: "Films" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText(
    "The rules changed, so the groups must be classified again.",
  );
  expect(screen.getByText("Earlier version")).toBeDefined();
  expect(screen.getByRole("button", { name: "Classify again" })).toBeDefined();
  await flush();
  expect(fake.count("group.classify")).toBe(1);
  expect(fake.count("group.rules.save")).toBe(1);
});

test("F36 FR3: a condition with no value is marked, Save is blocked with its reason, and Back is disabled while editing", async () => {
  mount();
  await screen.findByText("Movies + Archive");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const editor = await screen.findByRole("region", {
    name: "Edit group rules",
  });
  expect(screen.getByText("Not saved")).toBeDefined();
  expect(
    screen
      .getByRole("button", { name: "Back to folders" })
      .hasAttribute("disabled"),
  ).toBe(true);
  fireEvent.click(
    within(editor).getByRole("button", { name: "Add name condition" }),
  );
  // The text is the field's description and a visible copy under the row.
  expect(
    within(editor).getAllByText("Enter a value for the name condition."),
  ).toHaveLength(2);
  expect(
    within(editor)
      .getByRole("button", { name: "Save" })
      .hasAttribute("disabled"),
  ).toBe(true);
  expect(
    within(editor).getByText(
      "A condition has no value, so the rules cannot be saved.",
    ),
  ).toBeDefined();
  const values = within(editor).getAllByRole("textbox", {
    name: "Enter a value",
  });
  expect(values).toHaveLength(2);
  fireEvent.change(values[1]!, { target: { value: "mkv" } });
  expect(
    within(editor).queryByText("Enter a value for the name condition."),
  ).toBeNull();
  expect(
    within(editor)
      .getByRole("button", { name: "Save" })
      .hasAttribute("disabled"),
  ).toBe(false);
  fireEvent.click(within(editor).getByRole("button", { name: "Cancel" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Discard changes" }),
  );
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Edit group rules" }),
    ).toBeNull(),
  );
  expect(
    screen
      .getByRole("button", { name: "Back to folders" })
      .hasAttribute("disabled"),
  ).toBe(false);
});

test("F36 FR3: the preview starts from the applied result, recalculates a changed draft, and stores nothing", async () => {
  const DRAFT = "018f0000-0000-7000-8000-0000000000d1";
  const { fake } = await classified({
    "group.classify": (body: Body) => ({
      kind: "summary",
      requestSeq: body.requestSeq,
      summary:
        body.rules.kind === "draft"
          ? summary({
              classificationRef: DRAFT,
              rulesetVersion: 0,
              rulesetFingerprint: "ee",
              groups: summary().groups.map((row) =>
                row.groupId === MOVIES ? { ...row, memberCount: 9 } : row,
              ),
            })
          : summary(),
    }),
    "group.totals": (body: Body) =>
      body.classificationRef === DRAFT
        ? totals({
            classificationRef: DRAFT,
            groups: [
              { groupId: MOVIES, total: total("9437184") },
              { groupId: ARCHIVE, total: total("5242880") },
            ],
          })
        : totals(),
  });
  await screen.findByText("3.0 MiB");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const editor = await screen.findByRole("region", {
    name: "Edit group rules",
  });
  const preview = within(editor).getByRole("region", { name: "Preview" });
  // The applied result of the selected group is the preview until the draft changes.
  await within(preview).findByText("Members 3 items \u00b7 3.0 MiB");
  expect(
    within(preview).getByText(
      "Overlap with other groups 1 items \u00b7 Cannot decide 0 items",
    ),
  ).toBeDefined();
  expect(
    within(preview).getByText(
      "When you change a condition, the preview is recalculated within 2 seconds.",
    ),
  ).toBeDefined();
  const draftCalls = () =>
    fake.calls.filter(
      (call) =>
        call.name === "group.classify" && call.body.rules.kind === "draft",
    );
  expect(draftCalls()).toHaveLength(0);
  fireEvent.change(
    within(editor).getByRole("textbox", { name: "Enter a value" }),
    { target: { value: "mkv" } },
  );
  await within(preview).findByText("Members 9 items \u00b7 9.0 MiB");
  expect(draftCalls()).toHaveLength(1);
  const saved = fake.calls.find(
    (call) =>
      call.name === "group.classify" && call.body.rules.kind === "saved",
  )!;
  expect(draftCalls()[0]!.body.viewRef).not.toBe(saved.body.viewRef);
  expect(draftCalls()[0]!.body.rules.rules[0]).toMatchObject({
    groupId: MOVIES,
  });
  expect(fake.count("group.rules.save")).toBe(0);
});
