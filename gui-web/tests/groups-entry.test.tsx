import "./browse-scroll-fixture";
import {
  cleanup,
  configure,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { GroupsEntryAction } from "../src/features/groups/GroupsEntry";
import { GroupsSurface } from "../src/features/groups/GroupsSurface";
import { GroupsView } from "../src/features/groups/GroupsView";
import { useGroupsEntry } from "../src/features/groups/use-groups-entry";
import { AnalysisError } from "../src/ipc";
import type { DataSource } from "../src/ipc";
import {
  RESULT,
  SCOPE,
  fakeClient,
  flush,
  ruleset,
  standardHandlers,
  summary,
} from "./groups-fixtures";
import type { Body } from "./groups-fixtures";

afterEach(cleanup);
configure({ asyncUtilTimeout: 8000 });
vi.setConfig({ testTimeout: 30_000 });

const display = { system: "iec", unit: "auto" } as const;
const ENTRY = "View as virtual groups";
const SAMPLE_REASON = "Not available in the sample yet";

/** The browse screen's side: reports the browsed folder with the given data source. */
function Harness({
  client,
  dataSource,
}: {
  client: ReturnType<typeof fakeClient>["client"];
  dataSource: DataSource | undefined;
}) {
  const entry = useGroupsEntry(RESULT);
  return (
    <main>
      <GroupsEntryAction
        visible={entry.entryVisible(false)}
        sample={entry.sample}
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
              ...(dataSource === undefined ? {} : { dataSource }),
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

function browse(dataSource: DataSource | undefined) {
  const fake = fakeClient(standardHandlers());
  render(<Harness client={fake.client} dataSource={dataSource} />);
  return fake;
}

test("F36 FR2: the entry appears once a folder is browsed, opens the view, and closing returns focus to the entry button", async () => {
  const fake = fakeClient(standardHandlers());
  render(<Harness client={fake.client} dataSource="real" />);
  expect(screen.queryByRole("button", { name: ENTRY })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Browsed" }));
  const filter = screen.getByRole("textbox", {
    name: "Folder filter",
  }) as HTMLInputElement;
  filter.value = "kept";
  const entry = await screen.findByRole("button", { name: ENTRY });
  expect(entry.getAttribute("aria-disabled")).toBeNull();
  expect(screen.queryByText(SAMPLE_REASON)).toBeNull();
  entry.focus();
  fireEvent.click(entry);
  await screen.findByRole("heading", { name: "Group by my rules" });
  expect(screen.queryByRole("button", { name: ENTRY })).toBeNull();
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
    screen.getByRole("button", { name: ENTRY }),
  );
  expect(filter.value).toBe("kept");
});

test.each([
  ["absent", undefined],
  ["sample", "sample"],
] as const)(
  "F36 sample: a browsed result whose data source is %s keeps the entry disabled with its reason beside it",
  async (_name, dataSource) => {
    const fake = browse(dataSource);
    fireEvent.click(screen.getByRole("button", { name: "Browsed" }));
    const entry = await screen.findByRole("button", { name: ENTRY });
    const reason = screen.getByText(SAMPLE_REASON);
    // Disabled for the user but not removed from the tab order; the reason is its description.
    expect(entry.getAttribute("aria-disabled")).toBe("true");
    expect(entry.hasAttribute("disabled")).toBe(false);
    expect(entry.getAttribute("aria-describedby")).toBe(
      reason.parentElement!.id,
    );
    expect(entry.parentElement).toBe(reason.parentElement!.parentElement);
    entry.focus();
    expect(document.activeElement).toBe(entry);
    fireEvent.click(entry);
    await flush();
    expect(
      screen.queryByRole("heading", { name: "Group by my rules" }),
    ).toBeNull();
    expect(fake.calls).toHaveLength(0);
  },
);

test("F36 sample: before the browse screen reports its result there is no entry at all", () => {
  browse(undefined);
  expect(screen.queryByRole("button", { name: ENTRY })).toBeNull();
  expect(screen.queryByText(SAMPLE_REASON)).toBeNull();
});

function mount(extra: Parameters<typeof standardHandlers>[0] = {}) {
  const fake = fakeClient(standardHandlers(extra));
  render(
    <GroupsView
      client={fake.client}
      resultVersion={RESULT}
      scope={SCOPE}
      display={display}
      onClose={() => {}}
    />,
  );
  return fake;
}

test("F36 FR2: Back to folders appears once in the head, and only Member list is in the action row", async () => {
  mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "Start classifying" }),
  );
  await screen.findByRole("table", { name: "Virtual group summary" });
  expect(
    screen.getAllByRole("button", { name: "Back to folders" }),
  ).toHaveLength(1);
  const header = document.querySelector("header.du-review-surface__header")!;
  expect(
    within(header as HTMLElement).getByRole("button", {
      name: "Back to folders",
    }),
  ).toBeDefined();
  const footer = document.querySelector("footer.du-review-actions")!;
  const labels = Array.from(footer.querySelectorAll("button")).map(
    (button) => button.textContent,
  );
  expect(labels).toEqual(["Member list"]);
});

test("F36 #29 design review (1): with no saved rule the only next step is Edit rules and Start classifying is not offered", async () => {
  mount({ "group.rules.get": () => ruleset({ rules: [] }) });
  await screen.findByText("No rules are saved yet.");
  const edit = screen.getByRole("button", { name: "Edit rules" });
  expect(edit.className).toContain("du-action--primary");
  expect(
    screen.queryByRole("button", { name: "Start classifying" }),
  ).toBeNull();
  fireEvent.click(edit);
  await screen.findByRole("region", { name: "Edit group rules" });
  expect(screen.getByText("There are no groups. Add one.")).toBeDefined();
});

test("F36 FR3: while the editor is open the applied result stays on the left and Back is disabled", async () => {
  mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "Start classifying" }),
  );
  await screen.findByRole("table", { name: "Virtual group summary" });
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const editor = await screen.findByRole("region", {
    name: "Edit group rules",
  });
  const applied = screen.getByRole("table", { name: "Virtual group summary" });
  expect(editor.contains(applied)).toBe(false);
  expect(within(applied).getByText("Movies")).toBeDefined();
  expect(
    screen.getAllByRole("button", { name: "Back to folders" }),
  ).toHaveLength(1);
  expect(
    screen
      .getByRole("button", { name: "Back to folders" })
      .hasAttribute("disabled"),
  ).toBe(true);
});

test.each([
  [
    "predicate.unknown",
    "A condition is not registered, so the rules cannot be used.",
  ],
  [
    "group.rule_limit",
    "The number of groups or a group name is over the limit.",
  ],
  ["query.syntax_invalid", "A condition is not valid."],
])(
  "F36 FR3: a draft the engine refuses with %s is explained in words in the preview",
  async (reasonKey, sentence) => {
    mount({
      "group.classify": (body: Body) => {
        if (body.rules.kind === "draft")
          throw new AnalysisError("INVALID_ARGUMENT", reasonKey);
        return {
          kind: "summary",
          requestSeq: body.requestSeq,
          summary: summary(),
        };
      },
    });
    await screen.findByText("Movies + Archive");
    fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
    const editor = await screen.findByRole("region", {
      name: "Edit group rules",
    });
    fireEvent.change(
      within(editor).getAllByRole("textbox", { name: "Enter a value" })[0]!,
      { target: { value: "mkv" } },
    );
    const preview = within(editor).getByRole("region", { name: "Preview" });
    const notice = await within(preview).findByRole("alert");
    expect(notice.textContent).toContain(
      "The preview could not be calculated.",
    );
    expect(notice.textContent).toContain(sentence);
    expect(notice.textContent).not.toContain(reasonKey);
  },
);
