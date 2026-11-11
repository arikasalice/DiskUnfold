import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, expect, test, vi } from "vitest";
import { GroupMembersPresenter } from "../src/features/visualization/GroupMembersPresenter";
import type { GroupMembersPresenterProps } from "../src/features/visualization/GroupMembersPresenter";
import { modelFixture } from "../preview/model-fixture";
import { fixtureText } from "../preview/model-fixture";

afterEach(cleanup);

const row = modelFixture().page.response.rows[0]!;

function props(
  overrides: Partial<GroupMembersPresenterProps> = {},
): GroupMembersPresenterProps {
  return {
    returnLabel: "Back to graph",
    rows: [row],
    rangeLabel: "Rows 1–1 of 1",
    groupTotalLabel: "Group total: 12 KB",
    sizeBasisLabel: "Logical size",
    status: "ready",
    canRetry: false,
    hasPrevious: false,
    hasNext: false,
    text: fixtureText,
    onClose: vi.fn(),
    onPrevious: vi.fn(),
    onNext: vi.fn(),
    onRetry: vi.fn(),
    ...overrides,
  };
}

test("keeps heading and return action available during the first page load", () => {
  render(
    <GroupMembersPresenter
      {...props({ rows: undefined, status: "loading" })}
    />,
  );

  expect(
    screen.getByRole("heading", { name: "Other members", level: 2 }),
  ).toBeTruthy();
  expect(screen.getByRole("button", { name: "Back to graph" })).toBeTruthy();
  expect(
    screen.getByRole("progressbar", { name: "Loading members" }),
  ).toBeTruthy();
  expect(screen.queryByText(/No members in this range/)).toBeNull();
  expect(screen.queryByRole("table", { name: "Group members" })).toBeNull();
});

test("retains the last supplied rows during loading and reports the supplied range", () => {
  render(
    <GroupMembersPresenter
      {...props({ status: "loading", rangeLabel: "Rows 21–40 of 86" })}
    />,
  );

  expect(screen.getByText("Rows 21–40 of 86")).toBeTruthy();
  expect(
    screen.getByRole("table", { name: "Group members" }).textContent,
  ).toContain(row.displayName);
  expect(screen.queryByText(/No members in this range/)).toBeNull();
});

test("keeps the last supplied rows on error and retries only when requested", () => {
  const retry = vi.fn();
  render(
    <GroupMembersPresenter
      {...props({
        status: "error",
        canRetry: true,
        errorMessage: "Could not load rows 21–40.",
        rangeLabel: "Rows 1–20 of 86",
        hasNext: true,
        onRetry: retry,
      })}
    />,
  );

  expect(screen.getByRole("alert").textContent).toContain(
    "Could not load rows 21–40.",
  );
  expect(
    screen.getByRole("table", { name: "Group members" }).textContent,
  ).toContain(row.displayName);
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(retry).toHaveBeenCalledOnce();
});

test("shows an empty state only for a successfully supplied empty page", () => {
  const { rerender } = render(
    <GroupMembersPresenter {...props({ rows: [], status: "ready" })} />,
  );
  expect(screen.getByText("No members in this range.")).toBeTruthy();
  expect(screen.queryByRole("table", { name: "Group members" })).toBeNull();

  rerender(
    <GroupMembersPresenter
      {...props({ rows: [], status: "error", errorMessage: "Request failed." })}
    />,
  );
  expect(screen.queryByText("No members in this range.")).toBeNull();
  expect(screen.getByRole("alert").textContent).toContain("Request failed.");

  rerender(
    <GroupMembersPresenter {...props({ rows: undefined, status: "ready" })} />,
  );
  expect(screen.queryByText("No members in this range.")).toBeNull();
});

test("does not offer page retry unless the owner marks the error retryable", () => {
  render(
    <GroupMembersPresenter
      {...props({
        status: "error",
        errorMessage: "The model needs recovery.",
        canRetry: false,
      })}
    />,
  );

  expect(screen.getByRole("alert").textContent).toContain(
    "The model needs recovery.",
  );
  expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
});

test("does not infer an observation time or total when the owner has no labels", () => {
  const input = props({ rows: [], status: "ready" });
  delete input.rangeLabel;
  delete input.groupTotalLabel;
  delete input.sizeBasisLabel;
  delete input.observedAtLabel;
  const { container } = render(<GroupMembersPresenter {...input} />);

  expect(container.textContent).not.toMatch(/observed|total|unknown/i);
  expect(screen.getByText("No members in this range.")).toBeTruthy();
});

test("pages through injected callbacks and respects supplied range boundaries", () => {
  const previous = vi.fn();
  const next = vi.fn();
  render(
    <GroupMembersPresenter
      {...props({
        hasPrevious: true,
        hasNext: true,
        onPrevious: previous,
        onNext: next,
      })}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Previous range" }));
  fireEvent.click(screen.getByRole("button", { name: "Next range" }));
  expect(previous).toHaveBeenCalledOnce();
  expect(next).toHaveBeenCalledOnce();

  render(<GroupMembersPresenter {...props({ status: "loading" })} />);
  expect(
    screen
      .getAllByRole("button", { name: "Next range" })
      .at(-1)
      ?.hasAttribute("disabled"),
  ).toBe(true);
});

test("uses accessible heading, table, navigation and action semantics", async () => {
  const { container } = render(<GroupMembersPresenter {...props()} />);

  expect(
    screen.getByRole("navigation", { name: "Group member ranges" }),
  ).toBeTruthy();
  expect(screen.getByRole("table", { name: "Group members" })).toBeTruthy();
  expect((await axe.run(container)).violations).toEqual([]);
});
