import type { CallEnvelope, ResultEnvelope } from "./generated/contracts";

type NativeRequest = (payload: string) => Promise<string>;

interface NativeWindow extends Window {
  __diskunfoldRequest?: NativeRequest;
  webkit?: {
    messageHandlers?: { diskunfold?: { postMessage: NativeRequest } };
  };
}

export async function invokeNative(
  command: string,
  args: { envelope: CallEnvelope },
): Promise<ResultEnvelope> {
  const host = window as NativeWindow;
  const payload = JSON.stringify({ command, args });
  const handler = host.webkit?.messageHandlers?.diskunfold;
  if (host.top !== host)
    throw new Error("DiskUnfold IPC requires the main frame");
  const invoke = host.__diskunfoldRequest ?? handler?.postMessage.bind(handler);
  if (!invoke) throw new Error("DiskUnfold native IPC is unavailable");
  const result: ResultEnvelope = JSON.parse(await invoke(payload));
  if (
    result.correlationId !== args.envelope.correlationId ||
    result.name !== args.envelope.name
  ) {
    throw new Error("DiskUnfold IPC returned a mismatched response");
  }
  return result;
}
