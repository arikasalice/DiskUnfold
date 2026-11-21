import type { FolderQueryRow, SpaceTotal } from "../../ipc";
import { SIZE_UNIT_RULES } from "../../ipc";
import { formatByteUnit } from "../../i18n/bytes";
import type { SizeDisplay } from "../../i18n/bytes";
import type { LocaleText } from "../../i18n/presentation";

/**
 * Whether a size of a folder the scan did not enter, on another volume or
 * counted at another path, is a zero that stands for nothing counted in
 * this scope rather than an empty folder (F02 README "#133 갤러리 캡처 검수").
 */
export function notCounted(
  row: FolderQueryRow,
  value: FolderQueryRow["logicalSize"],
): boolean {
  const boundary = row.flags.boundary;
  return (
    boundary?.state === "present" &&
    (boundary.value === "other_volume" ||
      boundary.value === "already_counted") &&
    value.state === "present" &&
    value.value.state === "exact" &&
    BigInt(value.value.bytes) === 0n
  );
}

/**
 * Whether a lower bound of 0 bytes stands for nothing read: the folder was
 * not read, or a location inside it was not read. A folder whose counted
 * items are all empty has the same 0 but is shown as "At least 0 bytes".
 * An unread part makes the folder's size unknown in fact, so the cell says
 * so (F02 README #201 判定, F06 README 44.2 and 48.2).
 */
function unreadZeroBound(
  row: FolderQueryRow,
  value: FolderQueryRow["logicalSize"],
): boolean {
  return (
    value.state === "present" &&
    value.value.state === "lower_bound" &&
    BigInt(value.value.bytes) === 0n &&
    hasUnreadPart(row)
  );
}

/** Whether the folder itself or a location inside it was not read. */
export function hasUnreadPart(row: FolderQueryRow): boolean {
  const { unenumerated, unreadInside } = row.flags;
  return (
    (unenumerated.state === "present" && unenumerated.value) ||
    unreadInside?.state === "present"
  );
}

/**
 * A size cell: the readable size, "At least …" for a lower bound, and "Not
 * counted" for a zero that stands for nothing counted (see {@link notCounted}).
 */
export function size(
  row: FolderQueryRow,
  value: FolderQueryRow["logicalSize"],
  display: SizeDisplay,
  text: LocaleText,
): string {
  if (notCounted(row, value))
    return text("ui.browse.not_counted", "Not counted");
  if (unreadZeroBound(row, value))
    return text("ui.browse.unavailable", "Unavailable");
  if (value.state === "absent")
    return text("ui.browse.not_recorded", "Not recorded");
  if (value.state !== "present" || value.value.state === "unknown")
    return text("ui.browse.unavailable", "Unavailable");
  const lowerBound = value.value.state === "lower_bound";
  const readable = formatByteUnit(
    BigInt(value.value.bytes),
    SIZE_UNIT_RULES[display.system],
    SIZE_UNIT_RULES.decimalPlaces,
    display.unit,
    lowerBound,
    display.regional,
  );
  return `${lowerBound ? `${text("ui.browse.at_least", "At least")} ` : ""}${readable}`;
}

/** Whether the result-level identity limit explains a displayed physical-size lower bound. */
export function hasDedupCapacityLowerBound(total: SpaceTotal): boolean {
  return total.sizes.some(
    ({ size, measurementLimits }) =>
      (size.basis === "allocated" || size.basis === "unique_physical") &&
      size.state === "lower_bound" &&
      measurementLimits.includes("space.dedup_capacity"),
  );
}

/** The disabled reason of the copy action: the host's reason first, else "Unavailable now". */
export function copyDisabled(
  hostReason: string | undefined,
  unavailable: boolean,
  text: LocaleText,
): { disabledReason?: string } {
  if (hostReason !== undefined) return { disabledReason: hostReason };
  if (!unavailable) return {};
  return {
    disabledReason: text("ui.browse.unavailable_now", "Unavailable now"),
  };
}

/**
 * The "Scan status" cell of a row (F02 README #27 and "#133 「스캔 상태」
 * 열"): the marks the result records for the folder, joined with " · " when
 * more than one applies. Only a recorded mark is shown: "Contents not read"
 * for a folder whose opening or reading failed or that a cancellation or a
 * disconnection left, "Other volume" and "Counted at another path" for a
 * folder the scan did not enter. Anything else is blank, an unknown fact
 * too: a blank says no mark was recorded, not that the folder is complete,
 * and no row reads "Unknown". A boundary name this version does not know is
 * blank as well.
 */
export function scanStatus(row: FolderQueryRow, text: LocaleText): string {
  const { unenumerated, boundary, unreadInside } = row.flags;
  const labels: string[] = [];
  const notRead = unenumerated.state === "present" && unenumerated.value;
  if (notRead)
    labels.push(text("ui.browse.scan_status_not_read", "Contents not read"));
  if (
    row.kind === "directory" &&
    !notRead &&
    unreadInside?.state === "present"
  ) {
    const count = unreadInside.value.count;
    labels.push(
      count === undefined
        ? text(
            "ui.browse.scan_status_unread_inside_some",
            "Some locations inside not read",
          )
        : count === "1"
          ? text(
              "ui.browse.scan_status_unread_inside_one",
              "1 location inside not read",
            )
          : text(
              "ui.browse.scan_status_unread_inside_count",
              "{count} locations inside not read",
              { count },
            ),
    );
  }
  if (boundary?.state === "present" && boundary.value === "other_volume")
    labels.push(text("ui.browse.scan_status_other_volume", "Other volume"));
  if (boundary?.state === "present" && boundary.value === "already_counted")
    labels.push(
      text("ui.browse.scan_status_already_counted", "Counted at another path"),
    );
  return labels.join(" · ");
}

/** One notice for incomplete results and lower bounds with no row reason. */
export function browseNotice(
  incomplete: boolean,
  unexplainedLowerBounds: readonly string[],
  dedupCapacityLowerBound: boolean,
  text: LocaleText,
): string[] | undefined {
  const lines: string[] = [];
  if (incomplete)
    lines.push(text("ui.browse.results_incomplete", "Results are incomplete."));
  for (const key of new Set(unexplainedLowerBounds)) {
    lines.push(
      key === "scan.evidence_not_recorded"
        ? text(
            "ui.browse.lower_bound_evidence_not_recorded",
            "This result doesn't record whether folders were read completely, so some sizes and counts are minimums. Analyze again to see exact values.",
          )
        : key === "scan.evidence_budget_exceeded"
          ? text(
              "ui.browse.lower_bound_evidence_budget_exceeded",
              "There were too many folders to record whether each was read completely. Sizes and counts for those folders are minimums.",
            )
          : text(
              "ui.browse.lower_bound_unexplained",
              "This result doesn't record whether folders were read completely, so some sizes and counts are minimums.",
            ),
    );
  }
  if (dedupCapacityLowerBound)
    lines.push(
      text(
        "ui.browse.lower_bound_dedup_capacity",
        "There were too many files that may share content, so some weren't counted. Allocated and unique physical sizes are minimums.",
      ),
    );
  return lines.length > 0 ? lines : undefined;
}
