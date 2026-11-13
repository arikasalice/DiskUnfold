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
