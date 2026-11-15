import { useState } from "react";

type Pin = { version: string; updated: boolean; copy: boolean };
type Refusal = { version: string; copy: boolean };

/**
 * Keeps a dialog on the result version it opened with.
 *
 * The dialog shows what it read when it opened, even while the scan
 * publishes newer results; nothing asks the engine to keep that result.
 * When the dialog must read more and its result has been replaced, or when
 * a read is refused because the result was replaced, the whole dialog moves
 * to the newest result and `updated` turns on so it can say so once.
 * Closing and opening again always starts from the newest result.
 *
 * A copy the user pressed that was refused because the result was replaced
 * moves the dialog the same way and leaves `copy` on, so the dialog copies
 * once more on the newest result without another press. The dialog item
 * must name the same item in every result (the scope root does).
 *
 * `latest` is the newest result version the caller knows about.
 */
export function useDialogResult(latest: string) {
  const [pin, setPin] = useState<Pin>();
  const [staleAt, setStaleAt] = useState<Refusal>();
  // A refused read waits until a newer result is known, then moves there.
  if (pin && staleAt?.version === pin.version && latest !== staleAt.version) {
    setStaleAt(undefined);
    setPin({ version: latest, updated: true, copy: pin.copy || staleAt.copy });
  }
  function refuse(version: string, copy: boolean) {
    setStaleAt((earlier) => ({
      version,
      copy: copy || (earlier?.version === version && earlier.copy),
    }));
  }
  return {
    /** The version the open dialog reads, or undefined while it is closed. */
    version: pin?.version,
    /** Whether the open dialog moved to a newer result after it opened. */
    updated: pin?.updated ?? false,
    /** Whether the open dialog still owes the user a refused copy. */
    copy: pin?.copy ?? false,
    open() {
      setStaleAt(undefined);
      setPin({ version: latest, updated: false, copy: false });
    },
    close() {
      setStaleAt(undefined);
      setPin(undefined);
    },
    /** Moves to the newest result before a read the open values do not cover. */
    advance() {
      if (pin && pin.version !== latest)
        setPin({ ...pin, version: latest, updated: true });
    },
    /**
     * Records that a read of `version` was refused because it was replaced.
     * The dialog moves once a newer result than `version` is known; a report
     * about a version the dialog already left is ignored.
     */
    stale(version: string) {
      refuse(version, false);
    },
    /** Like `stale`, for a copy the user pressed; the dialog then owes it. */
    staleCopy(version: string) {
      refuse(version, true);
    },
    /** Records that the owed copy was tried, so it is never tried again. */
    copied() {
      setPin((current) =>
        current?.copy ? { ...current, copy: false } : current,
      );
    },
  };
}
