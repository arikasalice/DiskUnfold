import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, test } from "vitest";
import { useBrowseCopy } from "../src/features/browse/use-browse-copy";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import type { BrowseCopyPathOutcome, FolderQueryRow } from "../src/ipc";
import { localeSources } from "./locale-sources";

/*
 * The copy-path notice splits its sentence where the folder name stands and
 * draws the name there, so a sentence without the name's placeholder would
 * drop the name without any error. Every notice form is rendered from both
 * shipped bundles and from the copy written in code, and the name must be
 * found in each sentence.
 */

const NAME = "Alpha folder";
const unknown = { state: "unknown", reasonKey: "ref.unavailable" } as const;
const row: FolderQueryRow = {
  item: { resultVersion: "result", itemNo: 1 },
  displayName: { state: "present", value: NAME },
  kind: "directory",
  logicalSize: unknown,
  allocatedSize: { state: "absent" },
  modifiedAt: { state: "absent" },
  hasChildFolder: unknown,
  flags: {
    unenumerated: { state: "absent" },
    excluded: { state: "absent" },
  },
  correspondence: unknown,
};

const refusals: Extract<
  BrowseCopyPathOutcome,
  { outcome: "refused" }
>["reasonKey"][] = [
  "path.unrepresentable",
  "path.multiline",
  "clipboard.unsupported",
  "clipboard.failed",
];

/** Each copy answer, plus a copy still in progress and a failed call. */
const answers: (BrowseCopyPathOutcome | "pending" | "failed")[] = [
  "pending",
  { outcome: "copied", count: 1 },
  { outcome: "copied", count: 3 },
  ...refusals.map((reasonKey) => ({ outcome: "refused" as const, reasonKey })),
  "failed",
];

function answer(form: (typeof answers)[number]) {
  if (form === "pending")
    return new Promise<BrowseCopyPathOutcome>(() => undefined);
  if (form === "failed") return Promise.reject(new Error("Copy failed"));
  return Promise.resolve(form);
}

async function notice(text: LocaleText, form: (typeof answers)[number]) {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <LocaleTextContext.Provider value={text}>
      {children}
    </LocaleTextContext.Provider>
  );
  const { result, unmount } = renderHook(
    () =>
      useBrowseCopy(
        () => answer(form),
        0,
        () => 0,
        { current: null },
      ),
    { wrapper },
  );
  await act(async () => result.current.copy(row, null));
  const shown = {
    feedback: result.current.feedback,
    status: result.current.status,
  };
  unmount();
  return shown;
}

test.each(Object.entries(localeSources))(
  "every %s copy-path notice shows the folder name",
  async (_source, text) => {
    const statuses = new Set<string>();
    for (const form of answers) {
      const { feedback, status } = await notice(text, form);
      const label = JSON.stringify(form);
      expect(feedback?.name, label).toBe(NAME);
      expect(status, label).toContain(NAME);
      statuses.add(status);
    }
    // Each answer reached its own sentence.
    expect(statuses.size).toBe(answers.length);
  },
);
