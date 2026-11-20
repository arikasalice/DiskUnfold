import { interpolate } from "../i18n/presentation";
import type { LocaleText, TextValues } from "../i18n/presentation";
import {
  displayDecimal,
  displayInteger,
  displayTimestamp,
} from "../i18n/regional-format";
import type { RegionalFormat } from "../i18n/regional-format";
import { formatRegionalCapacity } from "../i18n/bytes";
import { SIZE_UNIT_RULES } from "../ipc";
import type { LocaleBundleBody } from "../ipc";
import type { LocaleView } from "./use-locale";
import type { makeLocaleReporter } from "./locale-missing";

type OsFamily = NonNullable<LocaleView["profile"]["osFamily"]>;

function localizedKey(
  bundle: LocaleBundleBody,
  key: string,
  osFamily: OsFamily,
) {
  // Only macOS and Linux read variants; "unknown" uses the base key, like an omitted family.
  const unix = osFamily === "macos" || osFamily === "linux";
  return [
    unix ? `${key}.${osFamily}` : undefined,
    unix ? `${key}.unix` : undefined,
    key,
  ].find(
    (candidate) =>
      candidate !== undefined && Object.hasOwn(bundle.messages, candidate),
  );
}

function plainMessage(
  bundle: LocaleBundleBody,
  key: string,
  osFamily: OsFamily,
) {
  const resolvedKey = localizedKey(bundle, key, osFamily);
  const message = resolvedKey ? bundle.messages[resolvedKey] : undefined;
  return message?.kind === "text" &&
    Object.keys(message.parameters).length === 0
    ? interpolate(message.template, () => "")
    : undefined;
}

/** Renders parameterless text only from a bundle supplied by the native host. */
export function createVerifiedBundleText(
  bundle: LocaleBundleBody,
  osFamily: OsFamily = "windows",
): LocaleText {
  return (key) => plainMessage(bundle, key, osFamily) ?? key;
}

function integer(value: string | bigint | undefined) {
  if (typeof value === "bigint") return value;
  return value !== undefined && /^-?(0|[1-9][0-9]*)$/.test(value)
    ? BigInt(value)
    : undefined;
}

function decimal(value: string | bigint | undefined, format: RegionalFormat) {
  if (typeof value === "bigint") return displayInteger(value, format);
  const parts = value?.match(/^(-?(?:0|[1-9][0-9]*))(?:\.([0-9]{1,38}))?$/);
  if (!parts?.[1]) return undefined;
  const fraction = parts[2] ?? "";
  return displayDecimal(BigInt(parts[1] + fraction), fraction.length, format);
}

function renderParameter(
  bundle: LocaleBundleBody,
  kind: string | undefined,
  value: string | bigint | undefined,
  format: RegionalFormat,
  osFamily: OsFamily,
) {
  if (kind === "path" || kind === "name")
    return typeof value === "string" ? value : undefined;
  if (kind === "message_key")
    return typeof value === "string"
      ? plainMessage(bundle, value, osFamily)
      : undefined;
  if (kind === "timestamp")
    return typeof value === "string"
      ? displayTimestamp(Date.parse(value), format)?.text
      : undefined;
  if (kind === "number") return decimal(value, format);
  const numeric = integer(value);
  if (numeric === undefined) return undefined;
  if (kind === "count") return displayInteger(numeric, format);
  if (kind === "size" && numeric >= 0n)
    return formatRegionalCapacity(
      numeric,
      SIZE_UNIT_RULES.iec,
      SIZE_UNIT_RULES.decimalPlaces,
      "auto",
      false,
      format,
    ).visual;
  return undefined;
}

function render(
  bundle: LocaleBundleBody,
  key: string,
  values: TextValues | undefined,
  format: RegionalFormat,
  osFamily: OsFamily,
) {
  const resolvedKey = localizedKey(bundle, key, osFamily);
  const message = resolvedKey ? bundle.messages[resolvedKey] : undefined;
  if (!message) return undefined;
  let incomplete = false;
  let template;
  if (message.kind === "text") template = message.template;
  else {
    const count = integer(values?.[message.parameter]);
    incomplete = count === undefined;
    const category =
      bundle.pluralRule === "en" && count === 1n ? "one" : "other";
    template = message.variants[category] ?? message.variants.other;
  }
  if (template === undefined) return undefined;
  const text = interpolate(template, (name) => {
    const parameter = renderParameter(
      bundle,
      message.parameters[name],
      values?.[name],
      format,
      osFamily,
    );
    if (parameter !== undefined) return parameter;
    incomplete = true;
    return plainMessage(bundle, "value.unknown", osFamily) ?? "value.unknown";
  });
  return { text, incomplete };
}

export function createLocaleText(
  view: LocaleView,
  format: RegionalFormat,
  report: ReturnType<typeof makeLocaleReporter>["note"],
): LocaleText {
  const { selected, baseline, language } = view;
  const osFamily = view.profile.osFamily ?? "windows";
  return (key, _fallback, values) => {
    const selectedText = render(selected, key, values, format, osFamily);
    const marker =
      plainMessage(selected, "ui.common.translation_unverified", osFamily) ??
      "Translation unverified";
    if (selectedText !== undefined)
      return selectedText.incomplete
        ? `${selectedText.text} (${marker})`
        : selectedText.text;
    const english = render(baseline, key, values, format, osFamily);
    report(language, key, english === undefined ? "key" : "fallback");
    return `${english?.text ?? key} (${marker})`;
  };
}
