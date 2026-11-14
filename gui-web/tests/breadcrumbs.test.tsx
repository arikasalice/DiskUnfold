import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Breadcrumbs } from "../src/components/breadcrumbs";

afterEach(cleanup);

test("keeps every ancestor accessible with its full name and current location text", () => {
  const navigate = vi.fn();
  const entries = Array.from({ length: 13 }, (_, index) => ({
    id: String(index),
    label: `Long project archive folder name ${index}`,
    current: index === 12,
    onPress: () => navigate(index),
  }));
  render(<Breadcrumbs label="Folder path" entries={entries} />);
  expect(screen.getAllByRole("button")).toHaveLength(12);
  fireEvent.click(screen.getByRole("button", { name: entries[3]!.label }));
  expect(navigate).toHaveBeenCalledWith(3);
  expect(
    screen.getByText(entries[12]!.label).getAttribute("aria-current"),
  ).toBe("location");
  expect(screen.queryByRole("button", { name: entries[12]!.label })).toBeNull();
});

test("reveals a changed path without resetting manual horizontal inspection on rerender", () => {
  const entries = [{ id: "first", label: "First location", current: true }];
  const { rerender } = render(
    <Breadcrumbs label="Folder path" entries={entries} />,
  );
  const viewport = screen.getByRole("navigation", { name: "Folder path" });
  Object.defineProperty(viewport, "scrollWidth", {
    configurable: true,
    value: 1200,
  });
  viewport.scrollLeft = 40;
  rerender(<Breadcrumbs label="Folder path" entries={[...entries]} />);
  expect(viewport.scrollLeft).toBe(40);
  rerender(
    <Breadcrumbs
      label="Folder path"
      entries={[{ id: "second", label: "Second location", current: true }]}
    />,
  );
  expect(viewport.scrollLeft).toBe(1200);
});

test("F02: every crumb marks edge white space on screen and reads the real name with its explanation", () => {
  const navigate = vi.fn();
  render(
    <Breadcrumbs
      label="Folder path"
      entries={[
        { id: "root", label: "범위 루트", onPress: () => navigate("root") },
        { id: "a", label: " 자료 ", onPress: () => navigate("a") },
        { id: "b", label: "문서", onPress: () => navigate("b") },
        { id: "c", label: "보고서 " },
        { id: "d", label: "초안", current: true },
      ]}
    />,
  );
  const spaced = screen.getByRole("button", {
    name: /^자료\s*, The name starts with 1 space, The name ends with 1 space$/,
  });
  const marks = spaced.querySelectorAll(".du-name__mark");
  expect(marks).toHaveLength(2);
  for (const mark of marks)
    expect(mark.getAttribute("aria-hidden")).toBe("true");
  expect(spaced.closest("li")?.getAttribute("title")).toBe(" 자료 ");
  fireEvent.click(spaced);
  expect(navigate).toHaveBeenCalledWith("a");

  // A translated root label and a plain name stay plain text.
  for (const name of ["범위 루트", "문서"]) {
    const plain = screen.getByRole("button", { name });
    expect(plain.textContent).toBe(name);
    expect(plain.querySelector(".du-name")).toBeNull();
  }

  // The title keeps the real label, including its trailing space.
  const text = document.querySelector<HTMLElement>('span[title="보고서 "]')!;
  expect(text.getAttribute("aria-current")).toBeNull();
  expect(
    text.querySelectorAll('.du-name__mark[aria-hidden="true"]'),
  ).toHaveLength(1);
  expect(text.textContent).toContain("The name ends with 1 space");
  const current = document.querySelector('[aria-current="location"]')!;
  expect(current.textContent).toBe("초안");
});
