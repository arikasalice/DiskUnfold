import "./browse-scroll-fixture";
import {
  act,
  cleanup,
  configure,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import axe from "axe-core";
import { afterEach, expect, test, vi } from "vitest";
import { GroupsEntryAction } from "../src/features/groups/GroupsEntry";
import { GroupsSurface } from "../src/features/groups/GroupsSurface";
import { GroupsView } from "../src/features/groups/GroupsView";
import { useGroupsEntry } from "../src/features/groups/use-groups-entry";
import { AnalysisError } from "../src/ipc";
import {
  CLASSIFICATION,
  ARCHIVE,
  MOVIES,
  NEWER,
  RESULT,
  SCOPE,
  deferred,
  fakeClient,
  flush,
  memberRow,
  ruleset,
  standardHandlers,
  summary,
  totals,
  unknownSize,
} from "./groups-fixtures";
import type { Body } from "./groups-fixtures";

afterEach(cleanup);
// Role queries with names walk a large grid; a loaded runner needs more than one second.
configure({ asyncUtilTimeout: 8000 });
vi.setConfig({ testTimeout: 30_000 });

const display = { system: "iec", unit: "auto" } as const;

function mount(
  extra: Parameters<typeof standardHandlers>[0] = {},
  onClose = () => {},
) {
  const fake = fakeClient(standardHandlers(extra));
  const view = render(
    <GroupsView
      client={fake.client}
      resultVersion={RESULT}
      scope={SCOPE}
      display={display}
      onClose={onClose}
    />,
  );
  return { fake, ...view };
}

async function classified(extra: Parameters<typeof standardHandlers>[0] = {}) {
  const mounted = mount(extra);
  fireEvent.click(
    await screen.findByRole("button", { name: "Start classifying" }),
  );
  await screen.findByRole("table", { name: "Virtual group summary" });
  return mounted;
}

test("F36 FR2: before classifying only the rules line and the start button show", async () => {
  const { fake } = mount();
  await screen.findByText("Movies, Archive");
  expect(
    screen.getByRole("heading", { name: "Group by my rules" }),
  ).toBeDefined();
  expect(
    screen.queryByRole("table", { name: "Virtual group summary" }),
  ).toBeNull();
  expect(
    screen.getByRole("button", { name: "Start classifying" }),
  ).toBeDefined();
  await flush();
  expect(fake.count("group.classify")).toBe(0);
});

test("F36 FR2: the summary lists groups with counts, sizes from the totals, and the classification's limits", async () => {
  await classified();
  const table = screen.getByRole("table", { name: "Virtual group summary" });
  expect(
    within(table).getByRole("button", { name: "Show members of Movies" }),
  ).toBeDefined();
  expect(
    within(table).getAllByText("Virtual group").length,
  ).toBeGreaterThanOrEqual(2);
  // Sizes come from group.totals, so they are not shown until that read answers.
  await waitFor(() => expect(within(table).getByText("3.0 MiB")).toBeDefined());
  expect(within(table).getByText("5.0 MiB")).toBeDefined();
  expect(within(table).getByText("No rule matched")).toBeDefined();
  expect(within(table).getByText("2 files could not be decided")).toBeDefined();
  expect(screen.getByText("The result is partial")).toBeDefined();
  expect(screen.getByRole("radio", { name: "Overlaps" })).toBeDefined();
  expect(
    screen.getByRole("region", { name: "Classification status" }),
  ).toBeDefined();
});

test("F36 FR6: a file in two groups is one row with a group-count badge and both group names, and its original path", async () => {
  await classified({
    "group.members": (body: Body) => ({
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
      start: body.start,
      total: 2,
      rows: [
        memberRow(1, { groups: [MOVIES, ARCHIVE] }),
        memberRow(2, {
          logicalSize: { state: "present", value: unknownSize },
          allocatedSize: { state: "unknown", reasonKey: "group.value_unknown" },
        }),
      ],
      sort: { id: "size", revision: 1 },
    }),
  });
  const rows = await screen.findAllByRole("row", { name: /file-\d\.mp4/ });
  expect(rows).toHaveLength(2);
  const shared = rows[0]!;
  expect(within(shared).getByText("Movies \u00b7 Archive")).toBeDefined();
  expect(within(shared).getByText("2 groups")).toBeDefined();
  expect(
    within(shared).getByRole("button", {
      name: "2 groups: show classification reasons",
    }),
  ).toBeDefined();
  expect(within(shared).getByText("C:\\Videos\\file-1.mp4")).toBeDefined();
  // A single-group row has no badge; unknown sizes are never written as zero.
  const single = rows[1]!;
  expect(within(single).queryByText(/groups$/)).toBeNull();
  expect(within(single).getAllByText("Unknown")).toHaveLength(2);
  expect(within(single).queryByText(/^0/)).toBeNull();
});

test("F36 FR7: totals read lower bounds as 'or more', unknown as unknown, and an undefined ratio as not calculable", async () => {
  const lower = {
    sizes: [
      {
        size: {
          basis: "logical" as const,
          evidence: "measured" as const,
          state: "lower_bound" as const,
          bytes: "7340032",
        },
        completeness: "partial" as const,
        measurementLimits: [],
      },
    ],
    completeness: "partial" as const,
  };
  await classified({
    "group.totals": () =>
      totals({
        union: lower,
        groups: [
          {
            groupId: MOVIES,
            total: {
              sizes: [
                {
                  size: unknownSize,
                  completeness: "partial",
                  measurementLimits: [],
                },
              ],
              completeness: "partial",
            },
          },
          { groupId: ARCHIVE, total: lower },
        ],
        ratios: [
          {
            state: "undefined",
            basis: "logical",
            reasonKey: "group.ratio_undefined",
          },
          {
            state: "value",
            basis: "allocated",
            numerator: "999",
            denominator: "1000",
          },
          {
            state: "value",
            basis: "unique_physical",
            numerator: "1",
            denominator: "100000",
          },
        ],
      }),
  });
  const totalsSection = await screen.findByRole("region", {
    name: "Combined total",
  });
  await within(totalsSection).findByText("7.0 MiB or more");
  expect(
    within(totalsSection).getByText("The measurement is not complete."),
  ).toBeDefined();
  expect(
    within(totalsSection).getByText(
      "Cannot be calculated. The ratio cannot be calculated.",
    ),
  ).toBeDefined();
  // The share is truncated, so 99.9% never reads as 100%, and a tiny share is never 0%.
  expect(within(totalsSection).getByText("99.9%")).toBeDefined();
  expect(within(totalsSection).getByText("less than 0.1%")).toBeDefined();
  const table = screen.getByRole("table", { name: "Virtual group summary" });
  expect(within(table).getByText("Unknown")).toBeDefined();
  expect(within(table).getByText("7.0 MiB or more")).toBeDefined();
});

test("F36 FR7: the reasons panel shows each rule's decision and each condition node", async () => {
  await classified({
    "group.members": (body: Body) => ({
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
      start: body.start,
      total: 1,
      rows: [memberRow(1, { groups: [MOVIES, ARCHIVE] })],
      sort: { id: "size", revision: 1 },
    }),
    "group.explain": (body: Body) => ({
      classificationRef: CLASSIFICATION,
      item: body.item,
      displayPath: { state: "present", value: "C:\\Videos\\file-1.mp4" },
      rules: [
        {
          groupId: MOVIES,
          decision: "match",
          nodes: [{ path: [], decision: "match" }],
        },
        {
          groupId: ARCHIVE,
          decision: "undecidable",
          reasonKey: "group.rule_unevaluable",
          nodes: [
            {
              path: [0, 1],
              predicate: { id: "role.video", revision: 2 },
              decision: "undecidable",
            },
          ],
        },
      ],
    }),
  });
  fireEvent.click(
    await screen.findByRole("button", {
      name: "2 groups: show classification reasons",
    }),
  );
  const panel = await screen.findByRole("region", {
    name: "Classification reasons",
  });
  await within(panel).findByText("Matches");
  expect(within(panel).getAllByText("Cannot decide").length).toBeGreaterThan(0);
  expect(
    within(panel).getByText(
      "The rule uses a condition that is not registered.",
    ),
  ).toBeDefined();
  fireEvent.click(
    within(panel).getByRole("button", { name: /Decision of each condition/ }),
  );
  expect(within(panel).getByText("Condition 1.2")).toBeDefined();
  expect(within(panel).getByText("role.video (2)")).toBeDefined();
});

test("F36 FR2: a newer result only offers an update; the user's update classifies the newer result", async () => {
  const { fake } = await classified({
    "group.classify": (body: Body) => ({
      kind: "summary",
      requestSeq: body.requestSeq,
      summary: summary({ resultVersion: body.resultVersion ?? RESULT }),
    }),
  });
  act(() => fake.emit("results.version_changed", { resultVersion: NEWER }));
  await screen.findByText("A newer result is available.");
  await flush();
  expect(fake.count("group.classify")).toBe(1);
  expect(
    screen.getByRole("table", { name: "Virtual group summary" }),
  ).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Classify again" }));
  await waitFor(() => expect(fake.count("group.classify")).toBe(2));
  expect(
    fake.calls.filter((call) => call.name === "group.classify")[1]!.body
      .resultVersion,
  ).toBe(NEWER);
});

test("F36 FR2: a running classification shows progress and can be cancelled", async () => {
  const { fake } = mount({
    "group.classify": (body: Body) => ({
      kind: "job",
      requestSeq: body.requestSeq,
      jobRef: "job-1",
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
    }),
    "job.get": () => ({
      jobRef: "job-1",
      kind: "group",
      state: "running",
      sequence: 1,
      resultVersion: null,
      reasonKey: null,
    }),
  });
  fireEvent.click(
    await screen.findByRole("button", { name: "Start classifying" }),
  );
  await screen.findByRole("button", { name: "Cancel classification" });
  expect(screen.getAllByText("Classifying…").length).toBeGreaterThan(0);
  fireEvent.click(
    screen.getByRole("button", { name: "Cancel classification" }),
  );
  await waitFor(() => expect(fake.count("group.cancel")).toBe(1));
  expect(fake.calls.find((call) => call.name === "group.cancel")!.body).toEqual(
    { kind: "job", jobRef: "job-1" },
  );
});

test("F36 FR2: an inapplicable source and an empty result read differently from an error", async () => {
  mount({
    "group.classify": (body: Body) => ({
      kind: "not_applicable",
      requestSeq: body.requestSeq,
      reasonKey: "group.source_unsupported",
    }),
  });
  fireEvent.click(
    await screen.findByRole("button", { name: "Start classifying" }),
  );
  await screen.findByText(/This result cannot be grouped\./);
  expect(
    screen.getByText(/This kind of result cannot be grouped\./),
  ).toBeDefined();
  cleanup();
  await classified({
    "group.classify": (body: Body) => ({
      kind: "summary",
      requestSeq: body.requestSeq,
      summary: summary({ matchedCount: 0, overlapCount: 0, groups: [] }),
    }),
  });
  expect(screen.getByText("No items match these rules.")).toBeDefined();
  expect(screen.queryByRole("alert")).toBeNull();
});

test("F36 FR2: a failed classification keeps the earlier result with an error and a retry", async () => {
  let fail = false;
  const { fake } = await classified({
    "group.classify": (body: Body) => {
      if (fail) throw new AnalysisError("BUSY", "db.busy", true);
      return {
        kind: "summary",
        requestSeq: body.requestSeq,
        summary: summary(),
      };
    },
  });
  fail = true;
  // The user takes the update offer; the new classification fails.
  act(() => fake.emit("results.version_changed", { resultVersion: NEWER }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Classify again" }),
  );
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("The classification failed.");
  expect(alert.textContent).toContain("The database is busy.");
  expect(screen.getByText("Previous classification")).toBeDefined();
  expect(
    screen.getByRole("table", { name: "Virtual group summary" }),
  ).toBeDefined();
  fail = false;
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  expect(fake.count("group.classify")).toBe(3);
});

test("F36 FR3: the editor cancels without a request; Escape closes only the editor and asks before dropping changes", async () => {
  const { fake } = mount();
  await screen.findByText("Movies, Archive");
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
  await screen.findByText("Movies, Archive");
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
  await screen.findByText("Movies, Archive");
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
  await screen.findByText("Movies, Archive");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  await screen.findByRole("region", { name: "Edit group rules" });
  fireEvent.click(screen.getByRole("radio", { name: /Archive/ }));
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

function Harness({
  client,
}: {
  client: ReturnType<typeof fakeClient>["client"];
}) {
  const entry = useGroupsEntry(RESULT);
  return (
    <main>
      <GroupsEntryAction
        visible={entry.entryVisible(false)}
        trigger={entry.trigger}
        onOpen={entry.show}
      />
      <div hidden={entry.open}>
        <input aria-label="Folder filter" defaultValue="" />
        <button
          onClick={() =>
            entry.onBrowsed(RESULT, {
              rootKind: "folder",
              location: SCOPE,
              page: { resultVersion: RESULT },
            })
          }
        >
          Browsed
        </button>
      </div>
      <GroupsSurface
        client={client}
        browseVersion={RESULT}
        entry={entry}
        display={display}
      />
    </main>
  );
}

test("F36 FR2: the entry appears once a folder is browsed, opens the view, and closing returns focus to the entry button", async () => {
  const fake = fakeClient(standardHandlers());
  render(<Harness client={fake.client} />);
  expect(
    screen.queryByRole("button", { name: "View as virtual groups" }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Browsed" }));
  const filter = screen.getByRole("textbox", {
    name: "Folder filter",
  }) as HTMLInputElement;
  filter.value = "kept";
  const entry = await screen.findByRole("button", {
    name: "View as virtual groups",
  });
  entry.focus();
  fireEvent.click(entry);
  await screen.findByRole("heading", { name: "Group by my rules" });
  expect(
    screen.queryByRole("button", { name: "View as virtual groups" }),
  ).toBeNull();
  // The folder view stays mounted underneath.
  expect(
    screen.getByLabelText("Folder filter", {
      selector: "input",
    }) as HTMLInputElement,
  ).toBe(filter);
  fireEvent.click(screen.getByRole("button", { name: "Back to folders" }));
  await waitFor(() =>
    expect(
      screen.queryByRole("heading", { name: "Group by my rules" }),
    ).toBeNull(),
  );
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "View as virtual groups" }),
  );
  expect(filter.value).toBe("kept");
});

test("F36 FR2: the screen has no file-change or value-editing controls and no path-like group names", async () => {
  await classified();
  const view = document.querySelector(".du-groups")!;
  const names = Array.from(view.querySelectorAll("button")).map(
    (button) => button.textContent ?? "",
  );
  expect(names.join("|")).not.toMatch(
    /delete|remove files|move|rename file|clean/i,
  );
  // Group names never appear in a path column.
  const paths = Array.from(view.querySelectorAll(".du-text--path")).map(
    (node) => node.textContent,
  );
  for (const path of paths) expect(path).not.toMatch(/^(Movies|Archive)$/);
});

test("F36 FR2: the loaded view passes an accessibility audit", async () => {
  await classified();
  await screen.findByText("3.0 MiB");
  const results = await axe.run(document.body, {
    rules: { "color-contrast": { enabled: false } },
  });
  expect(results.violations.map((violation) => violation.id)).toEqual([]);
});
