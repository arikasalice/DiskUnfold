import { ESLint } from "eslint";
import { expect, test } from "vitest";

// Loading the real ESLint configuration can exceed 5 seconds on a cold runner.
const options = { timeout: 30_000 };
const eslint = new ESLint();

async function rulesFor(source: string, filePath: string) {
  const [result] = await eslint.lintText(source, { filePath });
  return (result?.messages ?? [])
    .map(({ ruleId }) => ruleId)
    .filter((id): id is string => id?.startsWith("du/") === true);
}

const view = "src/features/analysis/Fixture.tsx";

test(
  "control-name rejects a control without an accessible name",
  options,
  async () => {
    expect(
      await rulesFor(
        'import { PathField } from "../../components"; export const A = () => <PathField value="" onChange={() => {}} />;',
        view,
      ),
    ).toEqual(["du/control-name"]);
    expect(
      await rulesFor(
        'import { Action } from "../../components"; export const A = () => <Action onPress={() => {}} />;',
        view,
      ),
    ).toEqual(["du/control-name"]);
    expect(
      await rulesFor(
        'import { Action } from "../../components"; export const A = () => <Action icon="copy" aria-label="Copy" onPress={() => {}} />;',
        view,
      ),
    ).toEqual(["du/control-name"]);
  },
);

test(
  "control-name rejects a name written as plain text instead of a message key",
  options,
  async () => {
    expect(
      await rulesFor(
        'import { ReviewField } from "../../components"; export const A = () => <ReviewField label="Note" value="" onChange={() => {}} description={d} />;',
        view,
      ),
    ).toEqual(["du/control-name"]);
  },
);

test("control-name accepts names that come from text()", options, async () => {
  expect(
    await rulesFor(
      'import { Action, PathField } from "../../components"; export const A = ({text}) => <><PathField label={text("ui.k", "Path")} value="" onChange={() => {}} /><Action onPress={() => {}}>{text("ui.go", "Go")}</Action><Action icon="copy" aria-label={text("ui.copy", "Copy")} onPress={() => {}} /></>;',
      view,
    ),
  ).toEqual([]);
});

test(
  "status-needs-shape-and-name rejects a colour-only status row and badge",
  options,
  async () => {
    expect(
      await rulesFor(
        'export const row = { id: "x", tone: "good", colorToken: "--du-good-text" };',
        "src/components/fixture-status.ts",
      ),
    ).toEqual(["du/status-needs-shape-and-name"]);
    expect(
      await rulesFor(
        'import { StatusBadge } from "../../components"; export const A = () => <StatusBadge tone="good" />;',
        view,
      ),
    ).toEqual(["du/status-needs-shape-and-name"]);
    expect(
      await rulesFor(
        'import { StatusBadge } from "../../components"; export const A = () => <StatusBadge tone="good" color="green">Done</StatusBadge>;',
        view,
      ),
    ).toEqual(["du/status-needs-shape-and-name"]);
  },
);

test(
  "status-needs-shape-and-name accepts a full status row and a named badge",
  options,
  async () => {
    expect(
      await rulesFor(
        'export const row = { id: "x", shape: "check", messageKey: "ui.k", colorToken: "--du-good-text" };',
        "src/components/fixture-status.ts",
      ),
    ).toEqual([]);
    expect(
      await rulesFor(
        'import { StatusBadge } from "../../components"; export const A = ({s, t}) => <><StatusBadge tone="good">{t}</StatusBadge><StatusBadge status={s} /></>;',
        view,
      ),
    ).toEqual([]);
  },
);

test(
  "no-physical-key rejects KeyboardEvent.code, physical key names and key codes",
  options,
  async () => {
    for (const body of [
      'if (event.code === "KeyQ") run();',
      'const bound = "KeyA";',
      'const digit = "Digit1";',
      "if (event.keyCode === 81) run();",
      "if (66 === e.which) run();",
    ]) {
      expect(
        new Set(
          await rulesFor(
            `export function onKey(event, e) { ${body} }`,
            "src/components/fixture-keys.ts",
          ),
        ),
      ).toEqual(new Set(["du/no-physical-key"]));
    }
  },
);

test(
  "no-physical-key accepts logical keys and the IME key code",
  options,
  async () => {
    expect(
      await rulesFor(
        'export function onKey(event) { if (event.key === "q" && event.ctrlKey) run(); if (event.keyCode === 229) skip(); }',
        "src/components/fixture-keys.ts",
      ),
    ).toEqual([]);
  },
);
