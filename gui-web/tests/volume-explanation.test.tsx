import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { SummaryHarness } from "./summary-fixture";
import { volumeReport } from "./summary-fixture";
import { AnalysisError, createAnalysisClient, windowsPath } from "../src/ipc";
import type { CallEnvelope, VolumeGapResponse } from "../src/ipc";

afterEach(cleanup);

const englishText: LocaleText = (_key, fallback) => fallback;
const localizedText: LocaleText = (key, fallback) =>
  key === "ui.analysis.volume_stale_result"
    ? "Abre el resultado más reciente para explicar el uso del volumen."
    : fallback;

test("volume problems use the active locale after the request finishes", async () => {
  const transport = vi.fn(async () => {
    throw new AnalysisError("STALE_VERSION", "result.requery_required");
  });
  const client = createAnalysisClient(transport);
  const props = {
    client,
    version: "selected-version",
    roots: [windowsPath("C:\\fixture")],
    display: { system: "iec", unit: "auto" } as const,
  };
  const view = render(
    <LocaleTextContext.Provider value={englishText}>
      <SummaryHarness live={false} {...props} />
    </LocaleTextContext.Provider>,
  );

  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  expect(
    await screen.findByText(
      "This result is no longer current. Open the latest result to explain its volume usage.",
    ),
  ).toBeTruthy();

  view.rerender(
    <LocaleTextContext.Provider value={localizedText}>
      <SummaryHarness live={false} {...props} />
    </LocaleTextContext.Provider>,
  );
  expect(
    screen.getByText(
      "Abre el resultado más reciente para explicar el uso del volumen.",
    ),
  ).toBeTruthy();
  expect(transport).toHaveBeenCalledTimes(1);
});

test.each([
  ["STALE_VERSION", "result.requery_required", /open the latest result/i],
  ["NOT_FOUND", "ref.unavailable", /run the analysis again/i],
  [
    "INVALID_ARGUMENT",
    "space.target_out_of_scope",
    /outside this analysis scope/i,
  ],
  ["INVALID_ARGUMENT", "command.body_invalid", /explanation is unavailable/i],
])(
  "explains %s %s without changing the selected result",
  async (code, reason, message) => {
    const transport = vi.fn(async () => {
      throw new AnalysisError(code, reason);
    });
    render(
      <SummaryHarness
        live={false}
        client={createAnalysisClient(transport)}
        version="selected-version"
        roots={[windowsPath("C:\\fixture")]}
        display={{ system: "iec", unit: "auto" }}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Explain volume usage" }),
    );
    expect(await screen.findByText(message)).toBeTruthy();
    expect(transport).toHaveBeenCalledTimes(1);
    expect(transport).toHaveBeenCalledWith(
      "du_call",
      expect.objectContaining({
        envelope: expect.objectContaining({
          name: "space.volume_gap",
          body: expect.objectContaining({ resultVersion: "selected-version" }),
        }),
      }),
    );
  },
);

test("F03 FR9: changing volume display units keeps raw bytes and evidence on the same report", async () => {
  const report = {
    resultVersion: "selected-version",
    requestSeq: 1,
    volumeId: "AQ",
    start: {
      state: "available",
      observedAt: null,
      totalBytes: "7500",
      usedBytes: "2500",
      freeBytes: "5000",
      evidence: "os_reported",
    },
    end: { state: "not_attempted" },
    allocated: {
      size: {
        basis: "allocated",
        state: "exact",
        bytes: "0",
        evidence: "measured",
      },
      completeness: "complete",
    },
    uniquePhysical: {
      size: {
        basis: "unique_physical",
        state: "exact",
        bytes: "0",
        evidence: "measured",
      },
      completeness: "complete",
    },
    factors: [
      {
        key: "gap.excluded",
        value: { state: "exact", bytes: "2500", evidence: "estimated" },
      },
    ],
    remainder: { state: "unknown", reasonKey: "space.data_insufficient" },
    osCategories: {
      state: "available",
      rows: [
        {
          key: "system",
          value: { state: "exact", bytes: "2500", evidence: "os_reported" },
        },
      ],
    },
  } satisfies VolumeGapResponse;
  const transport = vi.fn(
    async (_: string, { envelope }: { envelope: CallEnvelope }) => ({
      ...envelope,
      body: report,
    }),
  );
  const props = {
    client: createAnalysisClient(transport),
    version: report.resultVersion,
    roots: [windowsPath("C:\\fixture")],
  };
  const { rerender } = render(
    <SummaryHarness
      live={false}
      {...props}
      display={{ system: "si", unit: "auto" }}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Observation and difference details",
    }),
  );
  const observations = within(
    await screen.findByLabelText("Volume observations"),
  );
  const differences = within(screen.getByLabelText("Volume difference"));
  expect(observations.getByText("2.5 kB · 2,500 bytes")).toBeTruthy();
  expect(
    differences.getByText("2.5 kB · 2,500 bytes · Estimated"),
  ).toBeTruthy();

  rerender(
    <SummaryHarness
      live={false}
      {...props}
      display={{ system: "iec", unit: "auto" }}
    />,
  );
  expect(observations.getByText("2.4 KiB · 2,500 bytes")).toBeTruthy();
  expect(
    differences.getByText(
      "Unknown · 2.4 KiB · 2,500 bytes · Reported by the file system",
    ),
  ).toBeTruthy();
  // A category key is not shown.
  expect(differences.queryByText(/^system/)).toBeNull();

  rerender(
    <SummaryHarness
      live={false}
      {...props}
      display={{ system: "iec", unit: 0 }}
    />,
  );
  expect(observations.getByText("2,500 bytes")).toBeTruthy();
  expect(differences.getByText("2,500 bytes · Estimated")).toBeTruthy();
  // Unknown values stay concise; the reason appears once below the table.
  expect(differences.getByText("Unavailable")).toBeTruthy();
  expect(
    screen.getAllByText("Some space data could not be verified"),
  ).toHaveLength(1);
  expect(transport).toHaveBeenCalledTimes(1);
  expect(report.start.totalBytes).toBe("7500");
});

test("F03 #263 lists grouped system volumes and sums their out-of-scope amount", async () => {
  const report = {
    ...volumeReport("v1"),
    factors: [
      {
        key: "gap.out_of_scope",
        value: { state: "exact", bytes: "3584", evidence: "os_reported" },
      },
    ],
    otherVolumes: [
      {
        volumeName: "VM Store",
        role: "volume_role.vm",
        amount: { state: "exact", bytes: "1024", evidence: "os_reported" },
      },
      {
        volumeName: "Boot Prep",
        role: "volume_role.preboot",
        amount: { state: "exact", bytes: "1024", evidence: "os_reported" },
      },
      {
        volumeName: "Recovery Volume",
        role: "volume_role.recovery",
        amount: { state: "exact", bytes: "512", evidence: "os_reported" },
      },
      {
        volumeName: "Update Volume",
        role: "volume_role.update",
        amount: { state: "exact", bytes: "512", evidence: "os_reported" },
      },
      {
        volumeName: "Future Volume",
        role: "volume_role.future",
        amount: { state: "exact", bytes: "256", evidence: "os_reported" },
      },
      {
        volumeName: "Unclassified Volume",
        amount: { state: "exact", bytes: "256", evidence: "os_reported" },
      },
    ],
  } satisfies VolumeGapResponse;
  const transport = vi.fn(
    async (_: string, { envelope }: { envelope: CallEnvelope }) => ({
      ...envelope,
      body: {
        ...report,
        requestSeq: (envelope.body as { requestSeq: number }).requestSeq,
      },
    }),
  );
  render(
    <SummaryHarness
      live={false}
      client={createAnalysisClient(transport)}
      version={report.resultVersion}
      roots={[windowsPath("/")]}
      display={{ system: "iec", unit: "auto" }}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Observation and difference details",
    }),
  );
  const differences = within(await screen.findByLabelText("Volume difference"));
  expect(differences.getByText("Other system volumes")).toBeTruthy();
  expect(differences.getByText("3.5 KiB · 3,584 bytes")).toBeTruthy();
  expect(differences.getByText("Virtual memory (VM)")).toBeTruthy();
  expect(differences.getByText("Startup preparation (Preboot)")).toBeTruthy();
  expect(differences.getByText("Recovery")).toBeTruthy();
  expect(differences.getByText("Update")).toBeTruthy();
  expect(differences.getByText("Future Volume")).toBeTruthy();
  expect(differences.getByText("Unclassified Volume")).toBeTruthy();
});

test("F03 #263 leaves the out-of-scope label alone without otherVolumes", async () => {
  const report = {
    ...volumeReport("v2"),
    factors: [
      {
        key: "gap.out_of_scope",
        value: { state: "exact", bytes: "512", evidence: "os_reported" },
      },
    ],
  } satisfies VolumeGapResponse;
  const client = createAnalysisClient(
    async (_: string, { envelope }: { envelope: CallEnvelope }) => ({
      ...envelope,
      body: {
        ...report,
        requestSeq: (envelope.body as { requestSeq: number }).requestSeq,
      },
    }),
  );
  render(
    <SummaryHarness
      live={false}
      client={client}
      version={report.resultVersion}
      roots={[windowsPath("C:\\fixture")]}
      display={{ system: "iec", unit: "auto" }}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Observation and difference details",
    }),
  );
  const differences = within(await screen.findByLabelText("Volume difference"));
  expect(differences.getByText("Outside the analysis scope")).toBeTruthy();
  expect(differences.queryByText("Other system volumes")).toBeNull();
});

test("F03 #333 shows an unknown grouped total and one shared reason beneath the rows", async () => {
  const report = {
    ...volumeReport("v3"),
    factors: [
      {
        key: "gap.out_of_scope",
        value: { state: "unknown", reasonKey: "space.data_insufficient" },
      },
      {
        key: "gap.inaccessible",
        value: { state: "unknown", reasonKey: "space.data_insufficient" },
      },
    ],
    otherVolumes: [
      {
        volumeName: "VM Store",
        role: "volume_role.vm",
        amount: { state: "unknown", reasonKey: "space.data_insufficient" },
      },
      {
        volumeName: "Recovery Volume",
        role: "volume_role.recovery",
        amount: { state: "unknown", reasonKey: "space.data_insufficient" },
      },
    ],
  } satisfies VolumeGapResponse;
  const client = createAnalysisClient(
    async (_: string, { envelope }: { envelope: CallEnvelope }) => ({
      ...envelope,
      body: {
        ...report,
        requestSeq: (envelope.body as { requestSeq: number }).requestSeq,
      },
    }),
  );
  render(
    <SummaryHarness
      live={false}
      client={client}
      version={report.resultVersion}
      roots={[windowsPath("/")]}
      display={{ system: "iec", unit: "auto" }}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Observation and difference details",
    }),
  );
  const differences = within(await screen.findByLabelText("Volume difference"));
  expect(
    differences.getByText("Other system volumes").nextSibling?.textContent,
  ).toBe("Unavailable");
  expect(
    differences.getByText("Virtual memory (VM)").nextSibling?.textContent,
  ).toBe("Unavailable");
  expect(differences.getByText("Recovery").nextSibling?.textContent).toBe(
    "Unavailable",
  );
  expect(
    differences.queryByText(
      /Unavailable · Some space data could not be verified/,
    ),
  ).toBeNull();
  expect(
    screen.getAllByText("Some space data could not be verified"),
  ).toHaveLength(1);
});
