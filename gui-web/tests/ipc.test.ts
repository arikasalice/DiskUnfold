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
  const result: ResultEnvelope = { ...request, body: null };
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

test("requires a native bridge in the top-level window", async () => {
  await expect(createIpcClient().call(request)).rejects.toThrow(
    "native IPC is unavailable",
  );
  vi.stubGlobal("window", { top: {} });
  await expect(createIpcClient().call(request)).rejects.toThrow("main frame");
});
