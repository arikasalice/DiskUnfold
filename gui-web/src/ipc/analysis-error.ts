/** Carries a command failure while preserving retryability when the host knows it. */
export class AnalysisError extends Error {
  constructor(
    readonly code: string,
    readonly reasonKey: string,
    /** Retryability from the native error envelope, when available. */
    readonly retryable?: boolean,
  ) {
    super(reasonKey);
  }
}
