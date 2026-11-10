import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { VolumeExplanation } from "../src/features/analysis/VolumeExplanation";
import { AnalysisError, createAnalysisClient, windowsPath } from "../src/ipc";

afterEach(cleanup);

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
