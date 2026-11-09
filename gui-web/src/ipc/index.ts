import { invoke } from "@tauri-apps/api/core";
import type { CallEnvelope, ResultEnvelope } from "./generated/contracts";

export type { CallEnvelope, ResultEnvelope } from "./generated/contracts";

type Transport = (
  command: string,
  args: { envelope: CallEnvelope },
) => Promise<ResultEnvelope>;

export function createIpcClient(transport: Transport = invoke) {
  return {
    call(envelope: CallEnvelope) {
      return transport("du_call", { envelope });
    },
  };
}
