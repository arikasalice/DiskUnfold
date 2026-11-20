import type { LocaleText } from "../../i18n/presentation";

type Message = readonly [key: string, fallback: string];

/**
 * What an item detail shows for a value the scan could not read, by the
 * reason the scan recorded (F03 #239; design F02 README, item detail unknown
 * reasons). A note is the description line under the value (#199).
 */
const unreadValues = new Map<string, { value: Message; note?: Message }>([
  [
    "os.attribute_unsupported",
    {
      value: [
        "ui.inspection.not_provided_for_item",
        "Not provided for this item",
      ],
    },
  ],
  [
    "os.query_failed",
    {
      value: ["ui.analysis.unavailable", "Unavailable"],
      note: [
        "ui.inspection.query_failed_note",
        "The file system didn't return this value. It may be available if you analyze again.",
      ],
    },
  ],
  [
    "scan.boundary_other_volume",
    {
      value: ["ui.analysis.unavailable", "Unavailable"],
      note: [
        "ui.inspection.other_drive_note",
        "This is on another drive, so it wasn't included in this analysis.",
      ],
    },
  ],
]);

/** The value and description lines for a reason key; `undefined` for any other key. */
export function unreadValueText(
  key: string,
  text: LocaleText,
): { value: string; notes: string[] } | undefined {
  const entry = unreadValues.get(key);
  if (!entry) return undefined;
  return {
    value: text(...entry.value),
    notes: entry.note ? [text(...entry.note)] : [],
  };
}
