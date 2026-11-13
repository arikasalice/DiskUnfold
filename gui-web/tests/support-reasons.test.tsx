import axe from "axe-core";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { SupportScreen } from "../src/features/support/SupportScreen";
import {
  reasonText,
  statusLabel,
  supportCopy,
} from "../src/features/support/support-copy";
import { fakeHost, supportReport } from "./info-host";

afterEach(cleanup);

const baseline: LocaleText = (_key, fallback, values) =>
  interpolate(fallback, (name) => String(values?.[name] ?? ""));
const copy = supportCopy(baseline);

// Every reason key engine-api/src/support.rs can send.
const HOST_REASONS = [
  "support.env_unknown",
  "support.outside_verified",
  "support.matrix_unavailable",
  "support.matrix_invalid",
  "support.capability_unknown",
  "support.capability_limited",
];

test("gives each host reason its own words and only a foreign key the generic line", () => {
  const words = HOST_REASONS.map((key) => reasonText(key, copy));
  expect(new Set(words).size).toBe(HOST_REASONS.length);
  expect(words).not.toContain(copy.reasonOther);
  expect(reasonText("support.something_new", copy)).toBe(copy.reasonOther);
});

test("a computer outside the tested combinations reads as not tested, not as broken", async () => {
  const host = fakeHost({
    "app.support_info": () =>
      supportReport({
        combinationStatus: "not_verified",
        reasonKey: "support.outside_verified",
        securitySupport: null,
      }),
  });
  const { container } = render(<SupportScreen client={host.client} />);
  const status = await screen.findByRole("region", {
    name: "Combination status",
  });
  expect(within(status).getByText("Not tested")).toBeTruthy();
  expect(within(status).getByText(copy.reasonOutsideVerified)).toBeTruthy();
  expect(within(status).queryByText(copy.reasonOther)).toBeNull();
  expect(
    screen.getByRole("region", { name: "Tested combinations" }),
  ).toBeTruthy();
  expect((await axe.run(container)).violations).toEqual([]);
});

test("an unusable support table hides the combinations table and explains why", async () => {
  const host = fakeHost({
    "app.support_info": () =>
      supportReport({
        combinationStatus: "unknown",
        reasonKey: "support.matrix_invalid",
        securitySupport: null,
      }),
  });
  const { container } = render(<SupportScreen client={host.client} />);
  const header = await screen.findByRole("region", { name: "Support table" });
  expect(within(header).getByText(copy.reasonMatrixInvalid)).toBeTruthy();
  expect(screen.getAllByText(copy.reasonMatrixInvalid)).toHaveLength(1);
  expect(
    screen.queryByRole("region", { name: "Tested combinations" }),
  ).toBeNull();
  expect(screen.queryByText("Windows · 11 23H2 · Pro · x64")).toBeNull();
  expect((await axe.run(container)).violations).toEqual([]);
});

test("a feature area whose state could not be determined says so in plain words", async () => {
  const host = fakeHost({
    "app.support_info": () =>
      supportReport({
        features: [
          {
            area: "webview",
            supportTableStatus: "not_verified",
            currentCapability: "unknown",
            conditions: [],
            reasonKey: "support.capability_unknown",
          },
        ],
      }),
  });
  render(<SupportScreen client={host.client} />);
  const areas = within(
    await screen.findByRole("region", { name: "Feature areas" }),
  );
  expect(areas.getByText("Right now: Could not be determined")).toBeTruthy();
  expect(areas.getByText(copy.reasonCapabilityUnknown)).toBeTruthy();
});

test("names the worst status 'Unsupported found' as the design does", () => {
  expect(statusLabel("unsupported_found", copy)).toBe("Unsupported found");
});
