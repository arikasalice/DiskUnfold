import { createAnalysisClient } from "../src/ipc";
import type { ProductInfo, ResultEnvelope, SupportReport } from "../src/ipc";

type Handler = (body: Record<string, unknown>) => unknown;

/** Fails a command the way the native host reports an error envelope. */
export class HostFailure {
  constructor(readonly reasonKey = "support.internal_error") {}
}

/**
 * Builds an IPC client over an injected transport. Each command name maps to a
 * handler whose return value becomes the response body; a thrown value or a
 * HostFailure becomes an error envelope.
 */
export function fakeHost(handlers: Record<string, Handler>) {
  const calls: { name: string; body: Record<string, unknown> }[] = [];
  const transport = async (
    _command: string,
    { envelope }: { envelope: { name: string; body: unknown } },
  ): Promise<ResultEnvelope> => {
    const body = (envelope.body ?? {}) as Record<string, unknown>;
    calls.push({ name: envelope.name, body });
    const handler = handlers[envelope.name];
    if (!handler) throw new Error("unexpected command " + envelope.name);
    const value = await handler(body);
    if (value instanceof HostFailure) {
      return {
        ...envelope,
        body: null,
        error: {
          code: "INTERNAL",
          reasonKey: value.reasonKey,
          retryable: true,
        },
      } as unknown as ResultEnvelope;
    }
    return { ...envelope, body: value } as unknown as ResultEnvelope;
  };
  return {
    client: createAnalysisClient(
      transport as unknown as Parameters<typeof createAnalysisClient>[0],
    ),
    calls,
    count: (name: string) => calls.filter((call) => call.name === name).length,
  };
}

export const SHA = "ab".repeat(32);

export function productInfo(overrides: Partial<ProductInfo> = {}): ProductInfo {
  return {
    productVersion: "0.0.1",
    buildKind: "official",
    commit: "6cd1a30c9f",
    builtAtUtc: "2026-11-12T01:02:03Z",
    targetOs: "windows",
    targetCpu: "x64",
    installForm: "msi",
    installFormReasonKey: null,
    appDbFormat: 4,
    snapshotFormat: 2,
    supportMatrixRevision: "sm-7",
    supportMatrixAsOf: "2026-11-01",
    notices: [
      { kind: "LICENSE", available: true },
      { kind: "THIRD-PARTY-NOTICES", available: true },
    ],
    ...overrides,
  };
}

export function supportReport(
  overrides: Partial<SupportReport> = {},
): SupportReport {
  return {
    matrixRevision: "sm-7",
    asOf: "2026-11-01",
    environment: {
      osFamily: "windows",
      release: "11 23H2",
      edition: "Pro",
      cpu: "x64",
    },
    combinationStatus: "officially_verified",
    securitySupport: {
      source: "https://example.test/lifecycle",
      checkedOn: "2026-10-30",
      kind: "general",
    },
    features: [
      {
        area: "webview",
        supportTableStatus: "officially_verified",
        currentCapability: "limited",
        conditions: ["WebView2 runtime 120 or newer"],
        reasonKey: "support.capability_limited",
      },
      {
        area: "full_disk_access",
        supportTableStatus: "not_verified",
        currentCapability: "not_applicable",
        conditions: [],
        reasonKey: null,
      },
    ],
    combinations: [
      {
        identity: {
          osFamily: "windows",
          release: "11 23H2",
          edition: "Pro",
          cpu: "x64",
        },
        status: "officially_verified",
        securitySupport: null,
        verifiedDetails: ["MSI", "portable"],
        verificationRecords: ["run-1"],
        features: [],
      },
      {
        identity: {
          osFamily: "linux",
          release: "24.04",
          edition: null,
          cpu: "arm64",
        },
        status: "conditional",
        securitySupport: null,
        verifiedDetails: [],
        verificationRecords: [],
        features: [],
      },
    ],
    reasonKey: null,
    ...overrides,
  };
}
