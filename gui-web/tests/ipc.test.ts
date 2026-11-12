import { afterEach, expect, test, vi } from "vitest";
import {
  createIpcClient,
  type CallEnvelope,
  type ResultEnvelope,
} from "../src/ipc";

const request: CallEnvelope = {
  name: "app.product_info",
  contractVersion: 1,
  correlationId: "0195f856-ab83-7a59-9343-10d29775c9a9",
  body: {},
};

afterEach(() => vi.unstubAllGlobals());

test("calls the shared IPC bridge and preserves the response from an injected transport", async () => {
  const result: ResultEnvelope = { ...request, body: { name: "DiskUnfold" } };
  const transport = vi.fn().mockResolvedValue(result);

  await expect(createIpcClient(transport).call(request)).resolves.toEqual(
    result,
  );
  expect(transport).toHaveBeenCalledExactlyOnceWith("du_call", {
    envelope: request,
  });
});

test("preserves a transport failure for the caller", async () => {
  const error = new Error("bridge unavailable");
  const transport = vi.fn().mockRejectedValue(error);

  await expect(createIpcClient(transport).call(request)).rejects.toBe(error);
});

test("the default transport sends the native message and rejects unrelated replies", async () => {
  const result: ResultEnvelope & { futureField: string } = {
    ...request,
    body: null,
    futureField: "kept",
  };
  const post = vi
    .fn()
    .mockResolvedValueOnce(JSON.stringify(result))
    .mockResolvedValueOnce(
      JSON.stringify({ ...result, correlationId: "wrong" }),
    );
  vi.stubGlobal("__diskunfoldRequest", post);
  const client = createIpcClient();
  await expect(client.call(request)).resolves.toEqual(result);
  expect(post).toHaveBeenCalledWith(
    JSON.stringify({ command: "du_call", args: { envelope: request } }),
  );
  await expect(client.call(request)).rejects.toThrow("mismatched response");
});

test("validates a native error envelope and keeps additive fields", async () => {
  const response = {
    ...request,
    body: null,
    futureField: { accepted: true },
    error: {
      code: "INVALID_ARGUMENT",
      reasonKey: "command.body_invalid",
      messageKey: "command.body_invalid",
      correlationId: request.correlationId,
      retryable: false,
      itemRefs: [
        {
          resultVersion: request.correlationId,
          itemNo: 0,
          futureItemField: "accepted",
        },
      ],
      futureErrorField: "accepted",
    },
  };
  const post = vi.fn().mockResolvedValue(JSON.stringify(response));
  vi.stubGlobal("__diskunfoldRequest", post);

  await expect(createIpcClient().call(request)).resolves.toEqual(response);
});

test("accepts native errors with omitted optional item references", async () => {
  const response: ResultEnvelope = {
    ...request,
    body: null,
    error: {
      code: "BUSY",
      reasonKey: "db.busy",
      messageKey: "db.busy",
      correlationId: request.correlationId,
      retryable: true,
    },
  };
  const post = vi.fn().mockResolvedValue(JSON.stringify(response));
  vi.stubGlobal("__diskunfoldRequest", post);

  await expect(createIpcClient().call(request)).resolves.toEqual(response);
});

const invalidNativeReplies: Array<[string, unknown, RegExp]> = [
  ["null", null, /invalid response envelope/],
  ["a number primitive", 1, /invalid response envelope/],
  ["a string primitive", "response", /invalid response envelope/],
  ["a boolean primitive", true, /invalid response envelope/],
  ["an array", [], /invalid response envelope/],
  [
    "an envelope without a body",
    {
      name: request.name,
      contractVersion: request.contractVersion,
      correlationId: request.correlationId,
    },
    /invalid response envelope/,
  ],
  [
    "an unsupported contract version",
    { ...request, body: null, contractVersion: 2 },
    /mismatched response/,
  ],
  [
    "a different command name",
    { ...request, body: null, name: "other" },
    /mismatched response/,
  ],
  [
    "a different correlation ID",
    {
      ...request,
      body: null,
      correlationId: "0195f856-ab83-7a59-9343-10d29775c9aa",
    },
    /mismatched response/,
  ],
  [
    "an error with missing fields",
    { ...request, body: null, error: { code: "INTERNAL" } },
    /invalid error envelope/,
  ],
  [
    "an error with a different correlation ID",
    {
      ...request,
      body: null,
      error: {
        code: "INTERNAL",
        reasonKey: "support.internal_error",
        messageKey: "support.internal_error",
        correlationId: "0195f856-ab83-7a59-9343-10d29775c9aa",
        retryable: false,
      },
    },
    /invalid error envelope/,
  ],
  [
    "an error with an invalid item reference",
    {
      ...request,
      body: null,
      error: {
        code: "NOT_FOUND",
        reasonKey: "ref.unavailable",
        messageKey: "ref.unavailable",
        correlationId: request.correlationId,
        retryable: false,
        itemRefs: [{ resultVersion: request.correlationId, itemNo: -1 }],
      },
    },
    /invalid error envelope/,
  ],
];

test.each(invalidNativeReplies)(
  "rejects a native reply containing %s",
  async (_, reply, message) => {
    const post = vi.fn().mockResolvedValue(JSON.stringify(reply));
    vi.stubGlobal("__diskunfoldRequest", post);

    await expect(createIpcClient().call(request)).rejects.toThrow(message);
  },
);

test("rejects malformed native JSON", async () => {
  const post = vi.fn().mockResolvedValue("{not-json");
  vi.stubGlobal("__diskunfoldRequest", post);

  await expect(createIpcClient().call(request)).rejects.toThrow("invalid JSON");
});

test("applies response validation through the WebKit bridge fallback", async () => {
  const postMessage = vi.fn().mockResolvedValue("null");
  const nativeWindow: {
    top: unknown;
    webkit: {
      messageHandlers: { diskunfold: { postMessage: typeof postMessage } };
    };
  } = {
    top: undefined,
    webkit: { messageHandlers: { diskunfold: { postMessage } } },
  };
  nativeWindow.top = nativeWindow;
  vi.stubGlobal("window", nativeWindow);

  await expect(createIpcClient().call(request)).rejects.toThrow(
    "invalid response envelope",
  );
  expect(postMessage).toHaveBeenCalledExactlyOnceWith(
    JSON.stringify({ command: "du_call", args: { envelope: request } }),
  );
});

test("requires a native bridge in the top-level window", async () => {
  await expect(createIpcClient().call(request)).rejects.toThrow(
    "native IPC is unavailable",
  );
  vi.stubGlobal("window", { top: {} });
  await expect(createIpcClient().call(request)).rejects.toThrow("main frame");
});
