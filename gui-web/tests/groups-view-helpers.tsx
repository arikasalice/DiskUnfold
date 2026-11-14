import "./browse-scroll-fixture";
import {
  cleanup,
  configure,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, vi } from "vitest";
import { GroupsView } from "../src/features/groups/GroupsView";
import { RESULT, SCOPE, fakeClient, standardHandlers } from "./groups-fixtures";

afterEach(cleanup);
// Role queries with names walk a large grid; a loaded runner needs more than one second.
configure({ asyncUtilTimeout: 8000 });
vi.setConfig({ testTimeout: 30_000 });

export const display = { system: "iec", unit: "auto" } as const;

/** Mounts the screen for a real (observed) result. */
export function mount(
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

/** The tab (label element) of a category: the radio input holds no text of its own. */
export function tab(name: string): HTMLElement {
  return screen.getByRole("radio", { name }).closest("label")!;
}

export async function classified(
  extra: Parameters<typeof standardHandlers>[0] = {},
) {
  const mounted = mount(extra);
  fireEvent.click(
    await screen.findByRole("button", { name: "Start classifying" }),
  );
  await screen.findByRole("table", { name: "Virtual group summary" });
  return mounted;
}
