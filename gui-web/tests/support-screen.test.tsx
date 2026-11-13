import axe from "axe-core";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { SupportScreen } from "../src/features/support/SupportScreen";
import { HostFailure, fakeHost, supportReport } from "./info-host";

afterEach(cleanup);

test("shows the table header, environment, status, security basis and areas in separate sections", async () => {
  const host = fakeHost({ "app.support_info": () => supportReport() });
  const { container } = render(<SupportScreen client={host.client} />);
  const table = await screen.findByRole("region", { name: "Support table" });
  expect(within(table).getByText("sm-7")).toBeTruthy();
  expect(within(table).getByText("2026-11-01")).toBeTruthy();

  const env = screen.getByRole("region", { name: "This computer" });
  for (const value of ["Windows", "11 23H2", "Pro", "x64"]) {
    expect(within(env).getByText(value)).toBeTruthy();
  }

  const status = screen.getByRole("region", { name: "Combination status" });
  expect(within(status).getByText("Officially verified")).toBeTruthy();
  expect(within(status).queryByText(/lifecycle/)).toBeNull();

  const security = screen.getByRole("region", { name: "Security support" });
  expect(within(security).getByText("Standard support")).toBeTruthy();
  expect(
    within(security).getByText("https://example.test/lifecycle"),
  ).toBeTruthy();
  expect(within(security).getByText("2026-10-30")).toBeTruthy();

  expect((await axe.run(container)).violations).toEqual([]);
});

test("calls out a feature area whose current state differs from the support table", async () => {
  const host = fakeHost({ "app.support_info": () => supportReport() });
  render(<SupportScreen client={host.client} />);
  const areas = within(
    await screen.findByRole("region", { name: "Feature areas" }),
  );
  const webview = areas.getByText("WebView").closest("div") as HTMLElement;
  expect(within(webview).getByText("Right now: Limited")).toBeTruthy();
  expect(
    within(webview).getByText("Support table: Officially verified"),
  ).toBeTruthy();
  expect(
    within(webview).getByText(/differs from the support table/),
  ).toBeTruthy();
  expect(
    within(webview).getByText("Condition: WebView2 runtime 120 or newer"),
  ).toBeTruthy();
  expect(
    within(webview).getByText("This feature area is limited on this computer."),
  ).toBeTruthy();
  expect(webview.className).toContain("du-readout__emphasized");

  const access = areas
    .getByText("Full Disk Access")
    .closest("div") as HTMLElement;
  expect(within(access).getByText("Right now: Not applicable")).toBeTruthy();
  expect(access.className).not.toContain("du-readout__emphasized");
  expect(within(access).queryByText(/differs/)).toBeNull();
});

test("lists the embedded support combinations", async () => {
  const host = fakeHost({ "app.support_info": () => supportReport() });
  render(<SupportScreen client={host.client} />);
  const table = await screen.findByRole("table", {
    name: "Tested combinations",
  });
  const rows = within(table).getAllByRole("row");
  expect(rows).toHaveLength(3);
  expect(
    within(rows[1] as HTMLElement).getByText("Windows · 11 23H2 · Pro"),
  ).toBeTruthy();
  expect(
    within(rows[1] as HTMLElement).getByText("MSI, portable"),
  ).toBeTruthy();
  expect(
    within(rows[2] as HTMLElement).getByText("Linux · 24.04 · Not specified"),
  ).toBeTruthy();
  expect(
    within(rows[2] as HTMLElement).getByText("Verified with conditions"),
  ).toBeTruthy();
});

test("explains an unavailable support table without calling the system unsupported", async () => {
  const host = fakeHost({
    "app.support_info": () =>
      supportReport({
        matrixRevision: null,
        asOf: null,
        combinationStatus: "unknown",
        securitySupport: null,
        features: [],
        combinations: [],
        reasonKey: "support.matrix_unavailable",
      }),
  });
  const { container } = render(<SupportScreen client={host.client} />);
  expect(await screen.findAllByText(/no support table/)).toHaveLength(1);
  expect(screen.getByText(/does not mean it is unsupported/)).toBeTruthy();
  expect(screen.getAllByText("Not available")).toHaveLength(2);
  expect(screen.getByText("Could not be determined")).toBeTruthy();
  expect(screen.queryByRole("table")).toBeNull();
  expect(
    screen.getByText(
      "No security-support information is recorded for this system.",
    ),
  ).toBeTruthy();
  expect(
    screen.getByText("No feature-area details were reported."),
  ).toBeTruthy();
  expect((await axe.run(container)).violations).toEqual([]);
});

test("gives guidance when the environment could not be identified", async () => {
  const host = fakeHost({
    "app.support_info": () =>
      supportReport({
        environment: {
          osFamily: "unknown",
          release: null,
          edition: null,
          cpu: "unknown",
        },
        combinationStatus: "unknown",
        reasonKey: "support.env_unknown",
        securitySupport: null,
      }),
  });
  const { container } = render(<SupportScreen client={host.client} />);
  const env = await screen.findByRole("region", { name: "This computer" });
  expect(
    within(env).getByText(/could not identify this computer/),
  ).toBeTruthy();
  expect(within(env).getAllByText("Unknown")).toHaveLength(2);
  expect(within(env).getAllByText("Not detected")).toHaveLength(2);
  const status = screen.getByRole("region", { name: "Combination status" });
  expect(within(status).getByText("Could not be determined")).toBeTruthy();
  expect(within(status).queryByRole("status")).toBeNull();
  expect((await axe.run(container)).violations).toEqual([]);
});

test("treats values it does not know as undetermined rather than guessing", async () => {
  const base = supportReport();
  const host = fakeHost({
    "app.support_info": () => ({
      ...base,
      combinationStatus: "from_the_future",
      reasonKey: "support.something_new",
      features: [
        {
          area: "brand_new_area",
          supportTableStatus: "from_the_future",
          currentCapability: "from_the_future",
          conditions: [],
          reasonKey: "support.something_new",
        },
      ],
    }),
  });
  render(<SupportScreen client={host.client} />);
  const areas = within(
    await screen.findByRole("region", { name: "Feature areas" }),
  );
  expect(areas.getByText("Other feature area (brand_new_area)")).toBeTruthy();
  expect(areas.getByText("Right now: Could not be determined")).toBeTruthy();
  expect(
    areas.getByText("Support table: Could not be determined"),
  ).toBeTruthy();
  expect(
    screen.getAllByText(/reason that this version does not recognize/).length,
  ).toBeGreaterThan(0);
});

test("retries a failed read and keeps the last content when a refresh fails", async () => {
  let reads = 0;
  const host = fakeHost({
    "app.support_info": () => {
      reads += 1;
      return reads === 2 ? supportReport() : new HostFailure();
    },
  });
  render(<SupportScreen client={host.client} />);
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Support information could not be read.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  await screen.findByRole("region", { name: "Combination status" });
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "last successful read",
  );
  expect(
    screen.getByRole("region", { name: "Combination status" }),
  ).toBeTruthy();
  expect(host.count("app.support_info")).toBe(3);
});
