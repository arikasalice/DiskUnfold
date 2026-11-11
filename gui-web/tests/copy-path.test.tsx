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

test("a new item can copy while a late result cannot affect its pending request", async () => {
  const calls: CallEnvelope[] = [];
  const pending: ((result: ResultEnvelope) => void)[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope);
    return new Promise((resolve) => pending.push(resolve));
  });
  const view = render(<CopyPath client={client} item={item} path={path} />);
  fireEvent.click(screen.getByRole("button", { name: "Copy path" }));
  view.rerender(
    <CopyPath
      client={client}
      item={{ resultVersion: "result-b", itemNo: 2 }}
      path={path}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Copy path" }));
  expect(calls).toHaveLength(2);
  await act(async () =>
    pending[0]!(reply(calls[0]!, { outcome: "copied", count: 1 })),
  );
  expect(screen.getByRole("status").textContent).toBe("Copying path…");
  fireEvent.click(screen.getByRole("button", { name: "Copy path" }));
  expect(calls).toHaveLength(2);
  await act(async () =>
    pending[1]!(reply(calls[1]!, { outcome: "copied", count: 1 })),
  );
  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toBe("Copied 1 path."),
  );
});

test.each([
  ["clipboard.unsupported", "The clipboard is not supported."],
  ["clipboard.future", "Could not copy the path."],
])("announces refusal %s", async (reasonKey, expected) => {
  const client = createAnalysisClient(async (_, { envelope }) =>
    reply(envelope, { outcome: "refused", reasonKey }),
  );
  render(<CopyPath client={client} item={item} path={path} />);
  fireEvent.click(screen.getByRole("button", { name: "Copy path" }));
  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toBe(expected),
  );
});
