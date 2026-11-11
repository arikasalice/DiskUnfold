import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { GroupController } from "../src/features/visualization/group-controller";
import { GroupView } from "../src/features/visualization/GroupView";
import type { VizGroupRows } from "../src/ipc";

afterEach(cleanup);
test("opens the selected group and uses an announcement when no origin was verified", async () => {
  const frame = modelFixture().frame;
  const shape = frame.model.shapes[0]!;
  shape.itemRef = null;
  shape.groupRef = "group-a";
  const selection = {
    kind: "group" as const,
    groupRef: "group-a",
    modelRef: frame.model.modelRef,
    resultRevision: frame.resultRevision,
  };
  let resolve!: (value: VizGroupRows) => void;
  const rows = vi.fn(
    () =>
      new Promise<VizGroupRows>((done) => {
        resolve = done;
      }),
  );
  const controller = new GroupController({ rows });
  render(
    <GroupView
      controller={controller}
      frame={frame}
      selection={selection}
      text={fixtureText}
    />,
  );
  const open = screen.getByRole("button", { name: "View group members" });
  fireEvent.click(open);
  expect(rows).toHaveBeenCalledWith({
    modelRef: frame.model.modelRef,
    groupRef: "group-a",
    start: 0,
    count: 100,
  });
  await screen.findByRole("button", { name: "Back to visualization" });
  const back = screen.getByRole("button", { name: "Back to visualization" });
  back.focus();
  fireEvent.click(back);
  await screen.findByText(
    "The original group target is no longer available. Return to the visualization.",
  );
  expect(document.activeElement).toBe(screen.getByLabelText("Other members"));
  await act(async () =>
    resolve({
      rows: [],
      totalCount: 0,
      groupTotal: shape.size,
      sizeRange: { kind: "unknown" },
      resultRevision: frame.resultRevision,
      observedAt: frame.observedAt,
      completeness: frame.completeness,
    }),
  );
  await waitFor(() => expect(controller.getSnapshot().status).toBe("closed"));
  expect(
    screen.queryByRole("button", { name: "Back to visualization" }),
  ).toBeNull();
});

test("a stale frame never returns focus to its old group origin", async () => {
  const frame = modelFixture().frame;
  const next = modelFixture().frame;
  next.resultRevision = "new-result";
  const shape = frame.model.shapes[0]!;
  shape.itemRef = null;
  shape.groupRef = "group-a";
  const selection = {
    kind: "group" as const,
    groupRef: "group-a",
    modelRef: frame.model.modelRef,
    resultRevision: frame.resultRevision,
  };
  const controller = new GroupController({
    rows: vi.fn(() => new Promise<VizGroupRows>(() => {})),
  });
  const { rerender } = render(
    <>
      <button>Origin shape</button>
      <button>External control</button>
      <GroupView
        controller={controller}
        frame={frame}
        selection={selection}
        text={fixtureText}
      />
    </>,
  );
  fireEvent.click(screen.getByRole("button", { name: "View group members" }));
  await screen.findByRole("button", { name: "Back to visualization" });
  const external = screen.getByRole("button", { name: "External control" });
  external.focus();
  rerender(
    <>
      <button>Origin shape</button>
      <button>External control</button>
      <GroupView
        controller={controller}
        frame={next}
        selection={null}
        text={fixtureText}
      />
    </>,
  );
  expect(document.activeElement).toBe(external);
});

test("a replaced frame returns a focused group panel to the new view title", async () => {
  const frame = modelFixture().frame;
  const next = modelFixture().frame;
  next.resultRevision = "new-result";
  const groupRef = "row-group";
  const origin = document.createElement("button");
  const oldTitle = document.createElement("section");
  oldTitle.tabIndex = -1;
  const newTitle = document.createElement("section");
  newTitle.tabIndex = -1;
  newTitle.setAttribute("aria-label", "Storage visualization");
  document.body.append(oldTitle, newTitle);
  const returnTargetRef = { current: oldTitle as HTMLElement | null };
  const controller = new GroupController({
    rows: vi.fn().mockResolvedValue({
      rows: [],
      totalCount: 0,
      groupTotal: frame.model.shapes[0]!.size,
      sizeRange: { kind: "unknown" },
      resultRevision: frame.resultRevision,
      observedAt: frame.observedAt,
      completeness: frame.completeness,
    }),
  });
  await controller.open(
    {
      modelRef: frame.model.modelRef,
      resultRevision: frame.resultRevision,
      observedAt: frame.observedAt,
      completeness: frame.completeness,
    },
    { groupRef, shape: { groupRef, size: frame.model.shapes[0]!.size } },
  );
  const { rerender } = render(
    <GroupView
      controller={controller}
      frame={frame}
      selection={null}
      origin={{
        element: origin,
        kind: "row",
        modelRef: frame.model.modelRef,
        resultRevision: frame.resultRevision,
        observedAt: frame.observedAt,
        completeness: frame.completeness,
        groupRef,
        returnTarget: oldTitle,
      }}
      returnTargetRef={returnTargetRef}
      text={fixtureText}
    />,
  );
  const back = await screen.findByRole("button", {
    name: "Back to visualization",
  });
  back.focus();
  returnTargetRef.current = newTitle;
  rerender(
    <GroupView
      controller={controller}
      frame={next}
      selection={null}
      origin={{
        element: origin,
        kind: "row",
        modelRef: frame.model.modelRef,
        resultRevision: frame.resultRevision,
        observedAt: frame.observedAt,
        completeness: frame.completeness,
        groupRef,
        returnTarget: oldTitle,
      }}
      returnTargetRef={returnTargetRef}
      text={fixtureText}
    />,
  );
  await screen.findByText(
    "The original group target is no longer available. Return to the visualization.",
  );
  expect(
    screen
      .getAllByLabelText("Storage visualization")
      .find((element) => element.querySelector('[role="status"]')),
  ).toBeTruthy();
  expect(document.activeElement).toBe(newTitle);
  expect(document.activeElement).not.toBe(oldTitle);
  oldTitle.remove();
  newTitle.remove();
});

test("a virtualized away table row keeps the group open and closes to a safe notice", async () => {
  const frame = modelFixture().frame;
  const groupRef = "row-group";
  const origin = document.createElement("button");
  origin.textContent = "Open other group";
  document.body.append(origin);
  const controller = new GroupController({
    rows: vi.fn().mockResolvedValue({
      rows: [],
      totalCount: 0,
      groupTotal: frame.model.shapes[0]!.size,
      sizeRange: { kind: "unknown" },
      resultRevision: frame.resultRevision,
      observedAt: frame.observedAt,
      completeness: frame.completeness,
    }),
  });
  await controller.open(
    {
      modelRef: frame.model.modelRef,
      resultRevision: frame.resultRevision,
      observedAt: frame.observedAt,
      completeness: frame.completeness,
    },
    { groupRef, shape: { groupRef, size: frame.model.shapes[0]!.size } },
  );
  const { rerender } = render(
    <GroupView
      controller={controller}
      frame={frame}
      selection={null}
      origin={{
        element: origin,
        kind: "row",
        modelRef: frame.model.modelRef,
        resultRevision: frame.resultRevision,
        observedAt: frame.observedAt,
        completeness: frame.completeness,
        groupRef,
      }}
      text={fixtureText}
    />,
  );
  expect(
    await screen.findByRole("button", { name: "Back to visualization" }),
  ).toBeTruthy();
  origin.remove();
  rerender(
    <GroupView
      controller={controller}
      frame={frame}
      selection={null}
      origin={{
        element: origin,
        kind: "row",
        modelRef: frame.model.modelRef,
        resultRevision: frame.resultRevision,
        observedAt: frame.observedAt,
        completeness: frame.completeness,
        groupRef,
      }}
      text={fixtureText}
    />,
  );
  const back = screen.getByRole("button", { name: "Back to visualization" });
  back.focus();
  fireEvent.click(back);
  await screen.findByText(
    "The original group target is no longer available. Return to the visualization.",
  );
  expect(document.activeElement).toBe(screen.getByLabelText("Other members"));
});

test("a connected but disabled original target falls back to the group notice", async () => {
  const frame = modelFixture().frame;
  const groupRef = "row-group";
  const origin = document.createElement("button");
  origin.disabled = true;
  const controller = new GroupController({
    rows: vi.fn().mockResolvedValue({
      rows: [],
      totalCount: 0,
      groupTotal: frame.model.shapes[0]!.size,
      sizeRange: { kind: "unknown" },
      resultRevision: frame.resultRevision,
      observedAt: frame.observedAt,
      completeness: frame.completeness,
    }),
  });
  await controller.open(
    {
      modelRef: frame.model.modelRef,
      resultRevision: frame.resultRevision,
      observedAt: frame.observedAt,
      completeness: frame.completeness,
    },
    { groupRef, shape: { groupRef, size: frame.model.shapes[0]!.size } },
  );
  render(
    <GroupView
      controller={controller}
      frame={frame}
      selection={null}
      origin={{
        element: origin,
        kind: "row",
        modelRef: frame.model.modelRef,
        resultRevision: frame.resultRevision,
        observedAt: frame.observedAt,
        completeness: frame.completeness,
        groupRef,
      }}
      text={fixtureText}
    />,
  );
  const back = await screen.findByRole("button", {
    name: "Back to visualization",
  });
  back.focus();
  fireEvent.click(back);
  await screen.findByText(
    "The original group target is no longer available. Return to the visualization.",
  );
  expect(document.activeElement).toBe(screen.getByLabelText("Other members"));
});
