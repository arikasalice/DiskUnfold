import type { LocaleText } from "../../i18n/presentation";

type Entry = readonly [key: string, fallback: string];

const LIMITATIONS: Readonly<Record<string, Entry>> = {
  "group.limit.observed_record": [
    "ui.groups.limit_observed_record",
    "Based on the observation record, not the disk as it is now",
  ],
  "group.limit.partial": ["ui.groups.limit_partial", "The result is partial"],
  "group.limit.stale": [
    "ui.groups.limit_stale",
    "Some data may be out of date",
  ],
  "group.limit.undecidable": [
    "ui.groups.limit_undecidable",
    "Some files could not be decided",
  ],
  "group.limit.role_unconfirmed": [
    "ui.groups.limit_role_unconfirmed",
    "Roles are estimates",
  ],
  "group.limit.size_bound": [
    "ui.groups.limit_size_bound",
    "Some sizes are lower bounds",
  ],
  "condition.timezone_unavailable": [
    "ui.groups.limit_timezone",
    "The time zone was not available",
  ],
  "group.limit.ruleset_changed": [
    "ui.groups.limit_ruleset_changed",
    "The rules changed after this classification",
  ],
};

const REASONS: Readonly<Record<string, Entry>> = {
  "group.unknown": ["ui.groups.reason_unknown", "The group does not exist."],
  "group.rule_invalid": [
    "ui.groups.reason_rule_invalid",
    "A rule is not valid.",
  ],
  "group.name_duplicate": [
    "ui.groups.reason_name_duplicate",
    "Two groups have the same name.",
  ],
  "group.rule_limit": [
    "ui.groups.reason_rule_limit",
    "There are too many rules.",
  ],
  "group.rules_modified": [
    "ui.groups.reason_rules_modified",
    "The rules were changed elsewhere.",
  ],
  "group.result_expired": [
    "ui.groups.reason_result_expired",
    "The classification result is no longer available.",
  ],
  "group.source_unsupported": [
    "ui.groups.reason_source_unsupported",
    "This kind of result cannot be grouped.",
  ],
  "group.rule_unevaluable": [
    "ui.groups.reason_rule_unevaluable",
    "The rule uses a condition that is not registered.",
  ],
  "group.value_unknown": [
    "ui.groups.reason_value_unknown",
    "The value is unknown.",
  ],
  "group.ratio_undefined": [
    "ui.groups.reason_ratio_undefined",
    "The ratio cannot be calculated.",
  ],
  "group.ratio_inexact": [
    "ui.groups.reason_ratio_inexact",
    "The ratio is not exact.",
  ],
  "db.busy": [
    "ui.groups.reason_busy",
    "The database is busy. Try again in a moment.",
  ],
  "support.internal_error": [
    "ui.groups.reason_internal",
    "DiskUnfold hit an internal problem.",
  ],
};

function lookup(table: Readonly<Record<string, Entry>>, key: string) {
  return Object.hasOwn(table, key) ? table[key] : undefined;
}

/** Turns a limitation key into its label; an unknown key keeps its text visible. */
export function limitationLabel(
  key: string,
  count: number | undefined,
  text: LocaleText,
): string {
  const entry = lookup(LIMITATIONS, key);
  const label = entry
    ? text(entry[0], entry[1])
    : text("ui.groups.limit_unknown", "A limitation was reported: {key}", {
        key,
      });
  return count === undefined
    ? label
    : text("ui.groups.limit_with_count", "{label} ({count})", {
        label,
        count: String(count),
      });
}

/** Turns a reason key into a sentence; an unknown key is shown with its key. */
export function reasonLabel(
  reasonKey: string | undefined,
  text: LocaleText,
): string {
  if (reasonKey === undefined)
    return text("ui.groups.reason_none", "The reason is not known.");
  const entry = lookup(REASONS, reasonKey);
  return entry
    ? text(entry[0], entry[1])
    : text("ui.groups.reason_generic", "Reason: {key}", { key: reasonKey });
}
