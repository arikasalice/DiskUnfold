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
import { VolumeExplanation } from "../src/features/analysis/VolumeExplanation";
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
    resultVersion: "selected-version",
    roots: [windowsPath("C:\\fixture")],
    display: { system: "iec", unit: "auto" } as const,
  };
  const view = render(
    <LocaleTextContext.Provider value={englishText}>
      <VolumeExplanation {...props} />
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
      <VolumeExplanation {...props} />
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
  ["INVALID_ARGUMENT", "space.target_out_of_scope", /outside this scan/i],
  ["INVALID_ARGUMENT", "command.body_invalid", /explanation is unavailable/i],
])(
  "explains %s %s without changing the selected result",
  async (code, reason, message) => {
    const transport = vi.fn(async () => {
      throw new AnalysisError(code, reason);
    });
    render(
      <VolumeExplanation
        client={createAnalysisClient(transport)}
        resultVersion="selected-version"
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
    resultVersion: report.resultVersion,
    roots: [windowsPath("C:\\fixture")],
  };
  const { rerender } = render(
    <VolumeExplanation {...props} display={{ system: "si", unit: "auto" }} />,
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
    <VolumeExplanation {...props} display={{ system: "iec", unit: "auto" }} />,
  );
  expect(observations.getByText("2.4 KiB · 2,500 bytes")).toBeTruthy();
  expect(
    differences.getByText(
      "system · 2.4 KiB · 2,500 bytes · Reported by the file system",
    ),
  ).toBeTruthy();

  rerender(
    <VolumeExplanation {...props} display={{ system: "iec", unit: 0 }} />,
  );
  expect(observations.getByText("2,500 bytes")).toBeTruthy();
  expect(differences.getByText("2,500 bytes · Estimated")).toBeTruthy();
  expect(differences.getByText("Unavailable")).toBeTruthy();
  expect(transport).toHaveBeenCalledTimes(1);
  expect(report.start.totalBytes).toBe("7500");
});
