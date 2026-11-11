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
  act(() => button.focus());
  fireEvent.click(button);
  fireEvent.click(button);
  expect(calls).toHaveLength(1);
  expect(calls[0]?.name).toBe("browse.copy_path");
  expect(calls[0]?.body).toEqual({ resultVersion: "result-a", items: [item] });
  expect(screen.getByRole("status").textContent).toBe("Copying path…");
  expect(button.getAttribute("aria-disabled")).toBe("true");
  expect(button.hasAttribute("data-pending")).toBe(false);
  expect(document.activeElement).toBe(button);
  expect(document.querySelector('[role="log"] [aria-labelledby]')).toBeNull();
  await act(async () =>
    finish(reply(calls[0]!, { outcome: "copied", count: 1 })),
  );
  expect(screen.getByRole("status").textContent).toBe("Copied 1 path.");
  expect(button.getAttribute("aria-disabled")).not.toBe("true");
  expect(document.activeElement).toBe(button);
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

test("full path is available by disclosure and copy feedback can be dismissed", async () => {
  const fullPath = `C:/자료/${"long-folder/".repeat(12)}file.txt`;
  const client = createAnalysisClient(async (_, { envelope }) =>
    reply(envelope, { outcome: "refused", reasonKey: "clipboard.failed" }),
  );
  render(
    <CopyPath
      client={client}
      item={item}
      path={{ state: "present", value: fullPath }}
    />,
  );
  expect(screen.queryByText(fullPath)).toBeNull();
  const disclosure = screen.getByRole("button", { name: /Full path/ });
  fireEvent.click(disclosure);
  expect(screen.getByText(fullPath).getAttribute("tabindex")).toBe("0");
  const copy = screen.getByRole("button", { name: "Copy path" });
  fireEvent.click(copy);
  await screen.findByText("Could not copy to the clipboard.");
  const dismiss = screen.getByRole("button", { name: "Dismiss copy result" });
  act(() => dismiss.focus());
  fireEvent.click(dismiss);
  expect(screen.getByRole("status").textContent).toBe("");
  expect(document.activeElement).toBe(copy);
  fireEvent.click(copy);
  await screen.findByText("Could not copy to the clipboard.");
});

test("dismissing feedback does not move focus from another control", async () => {
  const client = createAnalysisClient(async (_, { envelope }) =>
    reply(envelope, { outcome: "copied", count: 1 }),
  );
  render(<CopyPath client={client} item={item} path={path} />);
  const copy = screen.getByRole("button", { name: "Copy path" });
  fireEvent.click(copy);
  await screen.findByText("Copied 1 path.");
  const disclosure = screen.getByRole("button", { name: /Full path/ });
  act(() => disclosure.focus());
  fireEvent.click(screen.getByRole("button", { name: "Dismiss copy result" }));
  expect(document.activeElement).toBe(disclosure);
});

test("unknown path hides internal reason while absent stays distinct", () => {
  const client = createAnalysisClient(async (_, { envelope }) =>
    reply(envelope, {}),
  );
  const view = render(
    <CopyPath
      client={client}
      item={item}
      path={{ state: "unknown", reasonKey: "internal.unmapped" }}
    />,
  );
  expect(screen.getByText("Unavailable in this scan")).toBeDefined();
  expect(screen.queryByText(/internal\.unmapped/)).toBeNull();
  view.rerender(
    <CopyPath client={client} item={item} path={{ state: "absent" }} />,
  );
  expect(screen.getByText("Not recorded")).toBeDefined();
});
