import { invokeNative } from "./bridge";
import type { CallEnvelope, ResultEnvelope } from "./generated/contracts";

export type { CallEnvelope, ResultEnvelope } from "./generated/contracts";

type Transport = (
  command: string,
  args: { envelope: CallEnvelope },
) => Promise<ResultEnvelope>;

export function createIpcClient(transport: Transport = invokeNative) {
  return {
    call(envelope: CallEnvelope) {
      return transport("du_call", { envelope });
    },
  };
}
