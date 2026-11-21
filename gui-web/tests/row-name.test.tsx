import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { RowName } from "../src/viz/RowName";

afterEach(cleanup);

test("#318: a folder row's name starts with the folder icon, as its bar does", () => {
  const folder = render(<RowName name="docs" item folder />);
  const icons = folder.container.querySelectorAll("svg");
  expect(icons).toHaveLength(1);
  // The bar label's size, in the row-name group that keeps 4 px to the name.
  expect(icons[0]!.getAttribute("width")).toBe("14");
  expect(folder.container.querySelector(".du-row-name")).not.toBeNull();
  expect(folder.container.textContent).toBe("docs");
  cleanup();
  const file = render(<RowName name="clip.mp4" item />);
  expect(file.container.querySelector("svg")).toBeNull();
  cleanup();
  // A group row keeps the engine's label as given, with no icon.
  const group = render(<RowName name="Other 5" item={false} folder />);
  expect(group.container.querySelector("svg")).toBeNull();
});
