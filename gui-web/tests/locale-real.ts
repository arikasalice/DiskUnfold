import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { LocaleBundleBody, LocaleProfile } from "../src/ipc";
import type { RegionalFormat } from "../src/i18n/regional-format";
import type { LocaleText } from "../src/i18n/presentation";
import { createLocaleText } from "../src/state/locale-text";

/*
 * The shipped English and Korean bundles read as the host would serve them, so a
 * test sees exactly what a user sees, including a key the bundle lacks.
 */

type Entry = [
  key: string,
  message: string | { parameter: string; variants: Record<string, string> },
  parameters: [name: string, kind: string][],
];

export type RealLocale = {
  language: "en" | "ko";
  body: LocaleBundleBody;
  keys: ReadonlySet<string>;
  regional: RegionalFormat;
  text: LocaleText;
};

function readBundle(language: "en" | "ko"): RealLocale {
  const data = JSON.parse(
    readFileSync(
      resolve(process.cwd(), `../user-env/i18n/${language}.json`),
      "utf8",
    ),
  ) as [
    string,
    string,
    LocaleBundleBody["formatTable"],
    string,
    string,
    Entry[],
  ];
  const messages: LocaleBundleBody["messages"] = {};
  for (const [key, message, parameters] of data[5]) {
    const kinds = Object.fromEntries(parameters) as Record<string, never>;
    messages[key] =
      typeof message === "string"
        ? { kind: "text", template: message, parameters: kinds }
        : {
            kind: "plural",
            parameter: message.parameter,
            variants: message.variants,
            parameters: kinds,
          };
  }
  const body: LocaleBundleBody = {
    messages,
    formatTable: data[2],
    pluralRule: data[3],
    reasonMessages: {},
    metadataRevision: "test",
  };
  const byteUnit = messages["ui.unit.byte"];
  const table = body.formatTable;
  const regional: RegionalFormat = {
    decimalSeparator: table.decimalSeparator,
    groupingSeparator: table.groupingSeparator,
    groupingSize: table.groupingSize,
    datePattern: table.datePatterns.lang_default ?? "yyyy-MM-dd'T'HH:mm:ss",
    dateFormat: "lang_default",
    groupNumbers: true,
    timeZonePolicy: "utc",
    localTimeZoneStatus: "unavailable",
    ...(byteUnit?.kind === "text" && { byteUnitText: byteUnit.template }),
  };
  const text = createLocaleText(
    {
      profile: {} as LocaleProfile,
      baseline: body,
      selected: body,
      language,
    },
    regional,
    () => undefined,
  );
  return {
    language,
    body,
    keys: new Set(Object.keys(messages)),
    regional,
    text,
  };
}

/** Both shipped bundles, by language. */
export const realLocales = { en: readBundle("en"), ko: readBundle("ko") };
