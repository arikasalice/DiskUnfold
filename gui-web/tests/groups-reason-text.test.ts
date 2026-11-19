import { expect, test } from "vitest";
import { reasonLabel } from "../src/features/groups/group-text";
import type { LocaleText } from "../src/i18n/presentation";

const keyed: LocaleText = (key, fallback) => `${key}|${fallback}`;

test("a cancelled request has its own reason text, distinct from an internal error", () => {
  expect(reasonLabel("request.cancelled", keyed)).toBe(
    "reason.request.cancelled.cause|The request was cancelled before it finished.",
  );
  expect(reasonLabel("support.internal_error", keyed)).toBe(
    "ui.groups.reason_internal|DiskUnfold hit an internal problem.",
  );
});
