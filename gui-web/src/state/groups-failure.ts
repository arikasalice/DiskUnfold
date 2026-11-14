import { AnalysisError } from "../ipc";

/** Extracts the reason of a failed call, if the host reported one. */
export function describeFailure(error: unknown): {
  reasonKey: string | undefined;
  retryable: boolean;
} {
  if (error instanceof AnalysisError)
    return {
      reasonKey: error.reasonKey,
      retryable: error.retryable === true,
    };
  return { reasonKey: undefined, retryable: false };
}
