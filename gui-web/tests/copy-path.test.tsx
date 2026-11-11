import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { CopyPath } from "../src/features/analysis/CopyPath";
import { createAnalysisClient } from "../src/ipc";
import type { CallEnvelope, ResultEnvelope } from "../src/ipc";

afterEach(cleanup);
const item = { resultVersion: "result-a", itemNo: 1 };
const path = { state: "present", value: "display-only" } as const;
function reply(envelope: CallEnvelope, body: unknown): ResultEnvelope {
  return {
    name: envelope.name,
    correlationId: envelope.correlationId,
    contractVersion: envelope.contractVersion,
    body,
  };
}

test("copy dispatches the pinned item once and announces the actual result", async () => {
  const calls: CallEnvelope[] = [];
  let finish!: (result: ResultEnvelope) => void;
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope);
    return new Promise((resolve) => {
      finish = resolve;
    });
  });
  render(<CopyPath client={client} item={item} path={path} />);
  const button = screen.getByRole("button", { name: "Copy path" });
  fireEvent.click(button);
  fireEvent.click(button);
  expect(calls).toHaveLength(1);
  expect(calls[0]?.name).toBe("browse.copy_path");
  expect(calls[0]?.body).toEqual({ resultVersion: "result-a", items: [item] });
  expect(screen.getByRole("status").textContent).toBe("Copying path…");
  await act(async () =>
    finish(reply(calls[0]!, { outcome: "copied", count: 1 })),
  );
  expect(screen.getByRole("status").textContent).toBe("Copied 1 path.");
});

test("refusals are announced and a late result cannot appear for another item", async () => {
  const calls: CallEnvelope[] = [];
  const pending: ((result: ResultEnvelope) => void)[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope);
    return new Promise((resolve) => pending.push(resolve));
  });
  const view = render(
    <CopyPath key="a" client={client} item={item} path={path} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Copy path" }));
  view.rerender(
    <CopyPath
      key="b"
      client={client}
      item={{ resultVersion: "result-b", itemNo: 2 }}
      path={path}
    />,
  );
  await act(async () =>
    pending[0]!(reply(calls[0]!, { outcome: "copied", count: 1 })),
  );
  expect(screen.getByRole("status").textContent).toBe("");
  fireEvent.click(screen.getByRole("button", { name: "Copy path" }));
  await act(async () =>
    pending[1]!(
      reply(calls[1]!, {
        outcome: "refused",
        reasonKey: "clipboard.unsupported",
      }),
    ),
  );
  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toBe(
      "The clipboard is not supported.",
    ),
  );
});
