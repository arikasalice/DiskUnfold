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

test("startup disk crumb exposes the real root path and marker in its tooltip and accessible name", () => {
  render(
    <Breadcrumbs
      label="Folder path"
      entries={[
        {
          id: "startup",
          label: "Macintosh HD",
          tooltip: "Macintosh HD · / · Startup disk",
          accessibleLabel: "Macintosh HD, /, Startup disk",
          onPress: () => undefined,
        },
      ]}
    />,
  );
  const button = screen.getByRole("button", {
    name: "Macintosh HD, /, Startup disk",
  });
  expect(button.closest("li")?.getAttribute("title")).toBe(
    "Macintosh HD · / · Startup disk",
  );
});

// #145, F02 README: the value uses the remaining width and a path breaks
// after a separator first; the end of a label is never cut.
// jsdom joins the parts around a <wbr> with a space when it computes an
// accessible name, so the label is found by its text here; ui-smoke logs the
// names UI Automation reads.
test("#145: a path label keeps its whole text and breaks after each separator", () => {
  const root = "C:\\Users\\runneradmin\\AppData\\Local\\Temp\\scope";
  render(
    <Breadcrumbs
      label="Folder path"
      entries={[
        { id: "root", label: root, onPress: () => undefined },
        { id: "photos", label: "Photos", current: true },
      ]}
    />,
  );
  const ancestor = screen.getAllByRole("button")[0]!;
  expect(ancestor.textContent).toBe(root);
  expect(ancestor.querySelectorAll("wbr")).toHaveLength(6);
  const current = screen.getByText("Photos");
  expect(current.getAttribute("aria-current")).toBe("location");
  expect(current.querySelectorAll("wbr")).toHaveLength(0);
  // A current location that is a path breaks the same way.
  render(
    <Breadcrumbs
      label="Scope root"
      entries={[{ id: "root", label: root, current: true }]}
    />,
  );
  const location = screen
    .getByRole("navigation", { name: "Scope root" })
    .querySelector('[aria-current="location"]')!;
  expect(location.textContent).toBe(root);
  expect(location.querySelectorAll("wbr")).toHaveLength(6);
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
