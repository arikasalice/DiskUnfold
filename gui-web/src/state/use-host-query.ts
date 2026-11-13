import { useCallback, useEffect, useState } from "react";

/** Result of a host read that keeps its last successful value. */
export type HostQuery<T> = {
  /** The last successfully read value, retained while refreshing or after a failed refresh. */
  data: T | undefined;
  /** A read is in flight. */
  loading: boolean;
  /** The most recent read failed. */
  failed: boolean;
  /** Reads again. */
  reload: () => void;
};

/**
 * Reads one idempotent host value when mounted and on demand.
 *
 * A failed read never discards an earlier successful value, so a screen can
 * keep showing the last valid content next to a retry action. The caller must
 * pass a stable `load` (for example through `useCallback`).
 */
export function useHostQuery<T>(load: () => Promise<T>): HostQuery<T> {
  const [data, setData] = useState<T>();
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setLoading(true);
    setFailed(false);
    load().then(
      (value) => {
        if (!active) return;
        setData(() => value);
        setLoading(false);
      },
      () => {
        if (!active) return;
        setFailed(true);
        setLoading(false);
      },
    );
    return () => {
      active = false;
    };
  }, [load, attempt]);
  const reload = useCallback(() => setAttempt((value) => value + 1), []);
  return { data, loading, failed, reload };
}
