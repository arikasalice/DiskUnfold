/** The two scan-state facts of a folder's flags that these rules read. */
type UnreadFlags = {
  unenumerated: { state: string; value?: unknown };
  unreadInside?: { state: string } | undefined;
};

/**
 * Whether a folder's flags say that it or a place inside it was not read. The one
 * rule of a folder row and of the details of that folder (F02 #347).
 */
export function hasUnreadFlags(flags: UnreadFlags): boolean {
  const { unenumerated, unreadInside } = flags;
  return (
    (unenumerated.state === "present" && unenumerated.value === true) ||
    unreadInside?.state === "present"
  );
}

/**
 * Whether a lower bound of 0 bytes in the details of a folder counts empty files: the
 * folder's flags are known and show nothing unread. Without flags (a file, a sample)
 * the 0 stands for nothing read.
 */
export function countedZeroOf(flags: UnreadFlags | undefined): boolean {
  return flags !== undefined && !hasUnreadFlags(flags);
}
