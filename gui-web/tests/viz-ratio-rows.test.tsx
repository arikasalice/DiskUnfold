import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { ModelRows } from "../src/features/visualization/VisualizationModelPresenter";
import { createProductionVizText, visualizationMessage } from "../src/viz";

afterEach(cleanup);

test.each(["en", "ko"] as const)(
  "the rows table shows the ratio as a percentage, with or without a model (%s)",
  (language) => {
    const text = createProductionVizText(
      (key, values) =>
        Object.entries(values ?? {}).reduce(
          (message, [name, value]) =>
            message.replace(`{${name}}`, String(value)),
          visualizationMessage(language, key),
        ),
      { system: "iec", unit: "auto" },
    );
    const { frame, page } = modelFixture();
    page.response.rows[0]!.ratio = "0.52022392576154932419";
    page.response.rows[1]!.ratio = null;
    for (const model of [frame.model, undefined]) {
      const { unmount } = render(
        <ModelRows page={page} model={model} text={text} />,
      );
      fireEvent.click(
        screen.getByRole("button", {
          name: visualizationMessage(language, "rowsFromGraph"),
        }),
      );
      expect(screen.getByText("52.0%")).toBeTruthy();
      expect(
        screen.getAllByText(visualizationMessage(language, "unavailableValue"))
          .length,
      ).toBeGreaterThan(0);
      expect(document.body.textContent).not.toContain("0.52022392576154932419");
      unmount();
    }
  },
);
