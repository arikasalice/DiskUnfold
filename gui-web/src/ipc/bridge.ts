import type {
  CallEnvelope,
  ErrorEnvelope,
  EventEnvelope,
  ItemRef,
  ResultEnvelope,
} from "./generated/contracts";

type NativeRequest = (payload: string) => Promise<string>;

interface NativeWindow extends Window {
  __diskunfoldRequest?: NativeRequest;
  __diskunfoldRecentScopes?: unknown;
  webkit?: {
    messageHandlers?: { diskunfold?: { postMessage: NativeRequest } };
  };
}

const MAX_U32 = 0xffff_ffff;
const UUID_V7_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isUint32(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= MAX_U32
  );
}

function isId(value: unknown): value is string {
  return typeof value === "string" && UUID_V7_PATTERN.test(value);
}

function isItemRef(value: unknown): value is ItemRef {
  return isRecord(value) && isId(value.resultVersion) && isUint32(value.itemNo);
}

function isResultEnvelope(value: unknown): value is ResultEnvelope {
  return (
    isRecord(value) &&
    typeof value.name === "string" &&
    isUint32(value.contractVersion) &&
    typeof value.correlationId === "string" &&
    Object.hasOwn(value, "body")
  );
}

function isErrorEnvelope(
  value: unknown,
  expectedCorrelationId: string,
): value is ErrorEnvelope {
  if (
    !isRecord(value) ||
    typeof value.code !== "string" ||
    typeof value.reasonKey !== "string" ||
    typeof value.messageKey !== "string" ||
    !isId(value.correlationId) ||
    value.correlationId !== expectedCorrelationId ||
    typeof value.retryable !== "boolean"
  ) {
    return false;
  }
  return (
    !Object.hasOwn(value, "itemRefs") ||
    (Array.isArray(value.itemRefs) && value.itemRefs.every(isItemRef))
  );
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

/** Reports the capability of the current desktop composition. */
export function hasRecentScopes(): boolean {
  return (
    typeof window !== "undefined" &&
    hasWindowsAnalysisBridge() &&
    (window as NativeWindow).__diskunfoldRecentScopes === true
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

/**
 * Sends a GUI command through the shared native bridge and validates its
 * response envelope before returning it. Unknown additive response fields are
 * preserved for forward-compatible consumers.
 *
 * @throws Error when the native bridge is unavailable, the response is
 * malformed, or its command identity does not match the request.
 */
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
  const rawResult = await invoke(payload);
  let parsedResult: unknown;
  try {
    parsedResult = JSON.parse(rawResult) as unknown;
  } catch {
    throw new Error("DiskUnfold IPC returned invalid JSON");
  }
  if (!isResultEnvelope(parsedResult)) {
    throw new Error("DiskUnfold IPC returned an invalid response envelope");
  }
  if (
    parsedResult.contractVersion !== 1 ||
    parsedResult.contractVersion !== args.envelope.contractVersion ||
    parsedResult.correlationId !== args.envelope.correlationId ||
    parsedResult.name !== args.envelope.name
  ) {
    throw new Error("DiskUnfold IPC returned a mismatched response");
  }
  if (!isId(parsedResult.correlationId)) {
    throw new Error("DiskUnfold IPC returned an invalid response envelope");
  }
  if (
    Object.hasOwn(parsedResult, "error") &&
    !isErrorEnvelope(parsedResult.error, parsedResult.correlationId)
  ) {
    throw new Error("DiskUnfold IPC returned an invalid error envelope");
  }
  return parsedResult;
}
