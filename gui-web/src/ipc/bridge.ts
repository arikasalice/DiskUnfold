import type {
  CallEnvelope,
  EventEnvelope,
  ResultEnvelope,
} from "./generated/contracts";

type NativeRequest = (payload: string) => Promise<string>;

interface NativeWindow extends Window {
  __diskunfoldRequest?: NativeRequest;
  webkit?: {
    messageHandlers?: { diskunfold?: { postMessage: NativeRequest } };
  };
}

/** Detects the embedded desktop bridge before presenting native analysis controls. */
export function hasWindowsAnalysisBridge(): boolean {
  const host = window as NativeWindow;
  return (
    host.top === host &&
    host.location.origin === "https://diskunfold.invalid" &&
    typeof host.__diskunfoldRequest === "function"
  );
}

/** Subscribes to validated event envelopes from the desktop host. */
export function subscribeNative(
  listener: (event: EventEnvelope) => void,
): () => void {
  const receive = (event: Event) => {
    if (!(event instanceof CustomEvent)) return;
    const envelope = event.detail as Partial<EventEnvelope> | null;
    if (
      envelope === null ||
      typeof envelope !== "object" ||
      envelope.contractVersion !== 1 ||
      typeof envelope.topic !== "string" ||
      typeof envelope.occurredAt !== "string" ||
      typeof envelope.seq !== "number" ||
      !Number.isSafeInteger(envelope.seq) ||
      envelope.seq < 1
    )
      return;
    listener(envelope as EventEnvelope);
  };
  window.addEventListener("du://event", receive);
  return () => window.removeEventListener("du://event", receive);
}

type NativeFolderDrop = { seq: number } & (
  | { pending: true }
  | { path: string }
  | { reason: "invalid_target" | "unsupported" }
);

/** Reads a local folder selection from the top-level native drop bridge. */
export function subscribeNativeFolderDrop(
  listener: (drop: NativeFolderDrop) => void,
): () => void {
  const receive = (event: Event) => {
    if (!(event instanceof CustomEvent)) return;
    const detail: unknown = event.detail;
    if (
      typeof detail !== "object" ||
      detail === null ||
      !("seq" in detail) ||
      typeof detail.seq !== "number" ||
      !Number.isSafeInteger(detail.seq) ||
      detail.seq < 1
    )
      return;
    const { seq } = detail;
    if ("pending" in detail && detail.pending === true) {
      listener({ seq, pending: true });
      return;
    }
    if (
      "path" in detail &&
      typeof detail.path === "string" &&
      detail.path.length > 0 &&
      detail.path.length <= 32_767 &&
      !detail.path.includes("\0")
    ) {
      listener({ seq, path: detail.path });
    } else if (
      "reason" in detail &&
      (detail.reason === "invalid_target" || detail.reason === "unsupported")
    ) {
      listener({ seq, reason: detail.reason });
    }
  };
  window.addEventListener("du://folder-drop", receive);
  return () => window.removeEventListener("du://folder-drop", receive);
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
