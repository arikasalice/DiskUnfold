import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import {
  createProductionVizText,
  explainReason,
  ModelEvidence,
} from "../src/viz";
import { bindVizMessage } from "../src/viz/use-viz-text";
import { scanIssueReasonText } from "../src/ipc";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { realLocales } from "./locale-real";

afterEach(cleanup);
const marker = "Translation unverified";

function evidence(
  text: Parameters<typeof ModelEvidence>[0]["text"],
  reasons: string[],
  staleness: string | null,
) {
  const { frame } = modelFixture();
  frame.model.summary = { ...frame.model.summary, reasonKeys: reasons };
  frame.model.staleness = { state: "unknown", reasonKey: staleness };
  render(<ModelEvidence frame={frame} text={text} />);
  fireEvent.click(screen.getByRole("button", { name: /\S/ }));
}

describe.each(["en", "ko"] as const)("reasons (%s)", (language) => {
  const locale = realLocales[language];
  const message = bindVizMessage(locale.text);
  const text = createProductionVizText(
    message,
    { system: "iec", unit: "auto", regional: locale.regional },
    (key) => scanIssueReasonText(key, locale.text),
  );

  test("F06 FR9.HD1.gui-web.DD1: reasons from the scan, the viz table and unknown keys share one resolver", () => {
    const denied = locale.text("ui.analysis.issue_reason_access_denied", "");
    const unknownScan = locale.text("ui.analysis.issue_unknown_reason", "");
    expect(explainReason("scan.path_denied", text)).toBe(denied);
    expect(explainReason("scan.something_new", text)).toBe(unknownScan);
    expect(explainReason("viz.staleness.unwatched", text)).toBe(
      message("stalenessUnwatched"),
    );
    expect(explainReason("viz.unavailable.no_baseline", text)).toBe(
      message("noBaseline"),
    );
    for (const key of ["something.else", "", "ui.visualization.group.other"])
      expect(explainReason(key, text), key).toBe(message("unknownReason"));
  });

  test("the evidence table shows summary reasons and the freshness reason as words", () => {
    evidence(
      text,
      ["scan.path_denied", "scan.something_new", "odd"],
      "viz.staleness.unwatched",
    );
    const shown = document.body.textContent ?? "";
    expect(shown).toContain(
      locale.text("ui.analysis.issue_reason_access_denied", ""),
    );
    expect(shown).toContain(
      locale.text("ui.analysis.issue_unknown_reason", ""),
    );
    expect(shown).toContain(message("unknownReason"));
    expect(shown).toContain(message("stalenessUnwatched"));
    for (const raw of [
      "scan.path_denied",
      "scan.something_new",
      "viz.staleness",
      marker,
    ])
      expect(shown).not.toContain(raw);
  });
});

test("F06 FR9.HD1.gui-web.DD1: without a scan reason table a scan key gets the general phrase", () => {
  expect(explainReason("scan.path_denied", fixtureText)).toBe(
    fixtureText.message("unknownReason"),
  );
  evidence(fixtureText, ["scan.path_denied"], null);
  expect(screen.getByText(fixtureText.message("unknownReason"))).toBeTruthy();
  expect(screen.queryByText("scan.path_denied")).toBeNull();
});
