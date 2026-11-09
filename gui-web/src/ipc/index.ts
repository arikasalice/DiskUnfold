import { invokeNative, subscribeNative } from "./bridge";
import type {
  AnalysisJobControl,
  AnalysisJobRequest,
  AnalysisJobStatus,
  CallEnvelope,
  EventEnvelope,
  Id,
  OsPath,
  ResultEnvelope,
  ScanStart,
  ScanStartRequest,
  ScopeReview,
  ScopeReviewRequest,
  SpaceTotal,
  SpaceTotalRequest,
  TargetList,
  TargetListRequest,
} from "./generated/contracts";

export type {
  AnalysisJobStatus,
  AnalysisTarget,
  EventEnvelope,
  JobState,
  OsPath,
  ScanStart,
  ScopeDraft,
  ScopeReview,
  SpaceTotal,
  SpaceTotalSize,
  TargetList,
  TargetStatusChanged,
} from "./generated/contracts";

type Commands = {
  "target.list": [TargetListRequest, TargetList];
  "scope.review": [ScopeReviewRequest, ScopeReview];
  "scan.start": [ScanStartRequest, ScanStart];
  "job.get": [AnalysisJobRequest, AnalysisJobStatus];
  "job.cancel": [AnalysisJobRequest, AnalysisJobControl];
  "space.total": [SpaceTotalRequest, SpaceTotal];
};

export interface AnalysisClient {
  call<K extends keyof Commands>(
    name: K,
    body: Commands[K][0],
  ): Promise<Commands[K][1]>;
  subscribe(listener: (event: EventEnvelope) => void): () => void;
}

export class AnalysisError extends Error {
  constructor(
    readonly code: string,
    readonly reasonKey: string,
  ) {
    super(reasonKey);
  }
}

/** Creates the UUID v7 request identifier required by the host envelope. */
function requestId(): Id {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  let timestamp = BigInt(Date.now());
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = Number(timestamp & 255n);
    timestamp >>= 8n;
  }
  const view = new DataView(bytes.buffer);
  view.setUint8(6, (view.getUint8(6) & 15) | 112);
  view.setUint8(8, (view.getUint8(8) & 63) | 128);
  const hex = Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20),
  ].join("-");
}

type Transport = (
  command: string,
  args: { envelope: CallEnvelope },
) => Promise<ResultEnvelope>;

/** Connects analysis commands and events through the versioned desktop transport. */
export function createAnalysisClient(
  transport: Transport = invokeNative,
): AnalysisClient {
  return {
    async call<K extends keyof Commands>(
      name: K,
      body: Commands[K][0],
    ): Promise<Commands[K][1]> {
      const envelope = {
        name,
        body,
        correlationId: requestId(),
        contractVersion: 1,
      };
      const result = await transport("du_call", { envelope });
      if (result.error)
        throw new AnalysisError(result.error.code, result.error.reasonKey);
      return result.body as Commands[K][1];
    },
    subscribe: subscribeNative,
  };
}

/** Encodes a path typed by the user without normalizing or resolving it. */
export function windowsPath(input: string): OsPath {
  let bytes = "";
  for (let index = 0; index < input.length; index += 1) {
    const unit = input.charCodeAt(index);
    bytes += String.fromCharCode(unit & 255, unit >> 8);
  }
  const raw = btoa(bytes)
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replace(/=+$/, "");
  return { encoding: "windows_utf16le", raw, display: input, lossy: false };
}

export { hasWindowsAnalysisBridge } from "./bridge";
export type { CallEnvelope, ResultEnvelope } from "./generated/contracts";
export function createIpcClient(transport: Transport = invokeNative) {
  return {
    call(envelope: CallEnvelope) {
      return transport("du_call", { envelope });
    },
  };
}
