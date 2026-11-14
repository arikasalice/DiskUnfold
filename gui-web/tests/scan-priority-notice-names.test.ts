import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import { interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { scanPriorityText } from "../src/ipc";
import type { ScanPriorityAnnouncement } from "../src/ipc";

/*
 * The "Analyze first" notice splits its sentence where each folder name
 * stands and draws the name there, so a sentence without the name's
 * placeholder would drop the name without any error. Every notice form is
 * rendered from both shipped bundles and from the copy written in code, and
 * each name passed to the sentence must appear in it.
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

const sources = { en: bundle("en"), ko: bundle("ko"), code: inCode };

const NAME = "Alpha folder";
const PREVIOUS = "Beta folder";

const forms: ScanPriorityAnnouncement[] = [
  ...[false, true].flatMap((applied) =>
    [false, true].flatMap((paused) =>
      [undefined, PREVIOUS].map((previous) => ({
        outcome: "accepted" as const,
        name: NAME,
        applied,
        paused,
        previous,
      })),
    ),
  ),
  ...(
    ["no_effect", "already_terminal", "stale_version", "failed"] as const
  ).map((outcome) => ({ outcome, name: NAME, applied: false, paused: false })),
];

test.each(Object.entries(sources))(
  "every %s notice sentence shows each folder name it is given",
  (_source, text) => {
    for (const form of forms) {
      const given: string[] = [];
      const recording: LocaleText = (key, fallback, values) => {
        for (const value of Object.values(values ?? {}))
          if (typeof value === "string") given.push(value);
        return text(key, fallback, values);
      };
      const sentence = scanPriorityText(form, recording);
      const label = JSON.stringify(form);
      for (const name of given) expect(sentence, label).toContain(name);
      if (form.outcome === "accepted") {
        expect(given, label).toContain(NAME);
        if (form.previous !== undefined)
          expect(given, label).toContain(PREVIOUS);
      }
    }
  },
);
