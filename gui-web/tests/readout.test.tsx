import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { NameText, Readout } from "../src/components";

afterEach(cleanup);

test("each row is a label followed by its value, with its kind as a class", () => {
  render(
    <Readout
      label="Build"
      rows={[
        { label: "Version", value: "0.1.0" },
        { label: "Commit", value: "c27fba25".repeat(5), identifier: true },
        { label: "Size", value: "4 GB", emphasized: true, notes: ["Exact"] },
      ]}
    />,
  );
  const list = screen.getByLabelText("Build");
  const rows = Array.from(list.children);
  expect(rows.map((row) => row.className)).toEqual([
    "",
    "du-readout__identifier",
    "du-readout__emphasized",
  ]);
  for (const row of rows)
    expect(Array.from(row.children, (child) => child.tagName)).toEqual([
      "DT",
      "DD",
    ]);
  expect(rows[2]?.querySelector("dd .du-readout__note")?.textContent).toBe(
    "Exact",
  );
});

test("a path value breaks after each separator and keeps its text", () => {
  const path = "C:\\Users\\민서\\문서\\2026 보고서.txt";
  render(
    <Readout
      label="Paths"
      rows={[{ label: "Scanned paths", value: <NameText path name={path} /> }]}
    />,
  );
  const value = screen.getByLabelText("Paths").querySelector("dd")!;
  expect(value.querySelector(".du-path")).not.toBeNull();
  expect(value.querySelectorAll("wbr")).toHaveLength(4);
  for (const wbr of value.querySelectorAll("wbr"))
    expect(wbr.previousSibling?.textContent?.endsWith("\\")).toBe(true);
  expect(value.textContent).toBe(path);
  expect(screen.getByText(path)).toBeDefined();
});

test("a path with a marked last component still breaks after its separators", () => {
  const path = "/home/민서/보고서 ";
  render(<NameText path name={path} />);
  const name = document.querySelector(".du-name.du-path")!;
  expect(name.querySelectorAll("wbr")).toHaveLength(3);
  expect(name.querySelectorAll(".du-name__mark")).toHaveLength(1);
  const withoutMarks = name.cloneNode(true) as Element;
  for (const extra of withoutMarks.querySelectorAll(
    ".du-name__mark, .du-visually-hidden",
  ))
    extra.remove();
  expect(withoutMarks.textContent).toBe(path);
});
