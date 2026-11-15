import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { ModelRows } from "../src/features/visualization/VisualizationModelPresenter";
import {
  createProductionVizText,
  plainRowName,
  rowName,
  shapeLabels,
} from "../src/viz";
import { bindVizMessage } from "../src/viz/use-viz-text";
import type { VizModel, VizRow, VizShape } from "../src/ipc";
import { modelFixture } from "../preview/model-fixture";
import { realLocales } from "./locale-real";

afterEach(cleanup);

const marker = "Translation unverified";
const OTHER = "ui.visualization.group.other";

function groupShape(groupRef: string): VizShape {
  const { frame } = modelFixture();
  return {
    ...frame.model.shapes[0]!,
    itemRef: null,
    groupRef,
    rootBranchRef: null,
    shapeRef: `shape-${groupRef}`,
  };
}

function modelWith(
  groups: string[],
  grouped: VizModel["summary"]["groupedItems"],
): VizModel {
  const { frame } = modelFixture();
  return {
    ...frame.model,
    shapes: [...frame.model.shapes, ...groups.map(groupShape)],
    summary: { ...frame.model.summary, groupedItems: grouped },
  };
}

function groupRow(patch: Partial<VizRow> = {}): VizRow {
  const { page } = modelFixture();
  const base = page.response.rows[0]!;
  return {
    ...base,
    itemRef: null,
    groupRef: "g1",
    displayName: OTHER,
    labelKey: OTHER,
    ...patch,
  };
}

const items = (state: "exact" | "lower_bound", value: string) =>
  ({ unit: "items", state, value }) as const;

describe.each(["en", "ko"] as const)("row names (%s)", (language) => {
  const locale = realLocales[language];
  const message = bindVizMessage(locale.text);
  const text = createProductionVizText(message, {
    system: "iec",
    unit: "auto",
    regional: locale.regional,
  });
  const other = (n: string, plus = false) =>
    language === "en"
      ? `Other (${n}${plus ? "+" : ""})`
      : `기타 ${n}개${plus ? " 이상" : ""}`;
  const unknownName = language === "en" ? "Other items" : "기타 항목";

  test("F06 FR7.HD1.app-host.DD1: the registered label key reads from the one group's item count", () => {
    const one = modelWith(["g1"], items("exact", "1234"));
    expect(rowName(groupRow(), one, text)).toBe(other("1,234"));
    expect(
      rowName(groupRow(), modelWith(["g1"], items("lower_bound", "7")), text),
    ).toBe(other("7", true));
    expect(
      rowName(
        groupRow(),
        modelWith(["g1"], { unit: "items", state: "unknown", reasonKey: "x" }),
        text,
      ),
    ).toBe(unknownName);
  });

  test("F06 FR7.HD1.app-host.DD1: more than one group, or a group the model does not draw, gets no count", () => {
    const two = modelWith(["g1", "g2"], items("exact", "9"));
    expect(rowName(groupRow(), two, text)).toBe(unknownName);
    const other1 = modelWith(["g2"], items("exact", "9"));
    expect(rowName(groupRow(), other1, text)).toBe(unknownName);
    const bytes = modelWith(["g1"], {
      unit: "bytes",
      state: "exact",
      value: "9",
    });
    expect(rowName(groupRow(), bytes, text)).toBe(unknownName);
  });

  test("F06 FR12.HD1.gui-web.DD1: an unregistered or missing key never reaches the screen", () => {
    const model = modelWith(["g1"], items("exact", "3"));
    for (const row of [
      groupRow({
        labelKey: "ui.visualization.nonsense",
        displayName: "ui.visualization.nonsense",
      }),
      groupRow({ labelKey: "scan.path_denied" }),
      groupRow({ labelKey: "unknown", displayName: "Readable but ignored" }),
      groupRow({ displayName: OTHER, labelKey: undefined as never }),
      groupRow({ displayName: "", labelKey: undefined as never }),
      groupRow({ displayName: "   ", labelKey: undefined as never }),
    ]) {
      const shown = rowName(
        {
          ...row,
          ...(row.labelKey === undefined ? { labelKey: undefined } : {}),
        } as VizRow,
        model,
        text,
      );
      expect(shown).toBe(unknownName);
      expect(shown).not.toContain(marker);
      expect(shown).not.toContain("ui.visualization");
    }
  });

  test("a name the supplier wrote stays, and an item row never takes its label key", () => {
    const model = modelWith(["g1"], items("exact", "3"));
    const bin = groupRow({ displayName: "0 B–1 kB" });
    delete (bin as { labelKey?: string }).labelKey;
    expect(rowName(bin, model, text)).toBe("0 B–1 kB");
    expect(plainRowName(bin)).toBe("0 B–1 kB");
    const { page } = modelFixture();
    const item: VizRow = {
      ...page.response.rows[0]!,
      displayName: "archive.tar.gz",
      labelKey: OTHER,
    };
    expect(rowName(item, model, text)).toBe("archive.tar.gz");
    expect(plainRowName(groupRow())).toBeNull();
  });

  test("F06 FR6.HD1.gui-web.DD1: the table, the shape's names and the row actions use the same name", () => {
    const model = modelWith(["g1"], items("exact", "3"));
    const row = groupRow();
    const shape = model.shapes.at(-1)!;
    const labels = shapeLabels(model, shape, row, text);
    expect(labels.accessibleLabel).toContain(other("3"));
    expect(labels.displayLabel).toContain(other("3"));
    expect(labels.accessibleLabel).not.toContain("ui.visualization");
    const { page } = modelFixture();
    render(
      <ModelRows
        page={{
          request: page.request,
          response: { ...page.response, rows: [row], totalCount: 1 },
        }}
        model={model}
        text={text}
        onOpenGroup={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /\S/ }));
    const name = other("3");
    expect(document.body.textContent).toContain(name);
    expect(document.body.textContent).not.toContain("ui.visualization");
    expect(
      screen.getByRole("button", {
        name: message("viewMembersOf", { name }),
      }),
    ).toBeTruthy();
  });
});
