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
test("opens the selected group's bound request, discards a late reply on close, and restores focus", async () => {
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
  fireEvent.click(
    screen.getByRole("button", { name: "Back to visualization" }),
  );
  expect(document.activeElement).toBe(open);
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
