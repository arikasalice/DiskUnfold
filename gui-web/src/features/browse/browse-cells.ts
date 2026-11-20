import type { FolderQueryRow } from "../../ipc";
import { SIZE_UNIT_RULES } from "../../ipc";
import { formatByteUnit } from "../../i18n/bytes";
import type { SizeDisplay } from "../../i18n/bytes";
import type { LocaleText } from "../../i18n/presentation";

/** A size cell: the readable size, "At least …" for a lower bound. */
export function size(
  value: FolderQueryRow["logicalSize"],
  display: SizeDisplay,
  text: LocaleText,
): string {
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
  const { unenumerated, boundary } = row.flags;
  const labels: string[] = [];
  if (unenumerated.state === "present" && unenumerated.value)
    labels.push(text("ui.browse.scan_status_not_read", "Contents not read"));
  if (boundary?.state === "present" && boundary.value === "other_volume")
    labels.push(text("ui.browse.scan_status_other_volume", "Other volume"));
  if (boundary?.state === "present" && boundary.value === "already_counted")
    labels.push(
      text("ui.browse.scan_status_already_counted", "Counted at another path"),
    );
  return labels.join(" · ");
}
