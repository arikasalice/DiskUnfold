import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";

/*
 * Message sources for tests that check what a sentence shows: both shipped
 * bundles and the copy written in code.
 */

type Entry = [key: string, text: unknown, parameters: unknown[]];

function bundle(language: string): LocaleText {
  const data = JSON.parse(
    readFileSync(
      resolve(process.cwd(), `../user-env/i18n/${language}.json`),
      "utf8",
    ),
  ) as unknown[];
  const messages = new Map(
    (data[5] as Entry[]).map(([key, value]) => [key, value]),
  );
  return (key, _fallback, values) => {
    const template = messages.get(key);
    if (typeof template !== "string")
      throw new Error(`${language} has no plain message for ${key}`);
    return interpolate(template, (name) => String(values?.[name] ?? ""));
  };
}

const inCode: LocaleText = (_key, fallback, values) =>
  interpolate(fallback, (name) => String(values?.[name] ?? ""));

/** The English and Korean bundles and the in-code copy, by name. */
export const localeSources = {
  en: bundle("en"),
  ko: bundle("ko"),
  code: inCode,
};
