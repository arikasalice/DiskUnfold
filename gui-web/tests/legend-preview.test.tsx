import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { TypeSlicesPreview } from "../preview/TypeSlicesPreview";
import { visualizationMessage } from "../src/viz";

afterEach(cleanup);

const say = (key: Parameters<typeof visualizationMessage>[1]) =>
  visualizationMessage("ko", key);

function legendRow(name: string) {
  const legend = screen.getByRole("listbox", { name: say("legendTitle") });
  const row = Array.from(legend.querySelectorAll("[role='option']")).find(
    (option) => option.textContent?.startsWith(name),
  );
  if (!(row instanceof HTMLElement)) throw new Error("no legend row " + name);
  return row;
}

function faded(container: HTMLElement) {
  const work = container.querySelector(".du-slice-stack");
  return {
    slices: Array.from(work?.querySelectorAll("[data-key]") ?? []).map(
      (part) => [
        part.getAttribute("data-key"),
        part.hasAttribute("data-dimmed"),
      ],
    ),
    shapes: container.querySelectorAll(".du-geometry-shape[data-dimmed]")
      .length,
  };
}

test("README 46.1, #383: resting the pointer on a legend row previews its type without choosing it", () => {
  const { container } = render(<TypeSlicesPreview scene="bar" />);
  const video = legendRow(say("legend.type.video"));
  act(() => {
    fireEvent.pointerEnter(video, { pointerType: "mouse" });
    fireEvent.mouseEnter(video);
  });
  const during = faded(container);
  expect(during.slices).toEqual([
    ["type.document", true],
    ["type.image", true],
    ["type.video", false],
  ]);
  // The document file has no video: it fades whole.
  expect(during.shapes).toBeGreaterThan(0);
  // A preview is not a choice: no chip, no narrowing, no selected row.
  expect(screen.queryByText(/×$/u)).toBeNull();
  expect(video.getAttribute("aria-selected")).not.toBe("true");
  act(() => {
    fireEvent.pointerLeave(video, { pointerType: "mouse" });
    fireEvent.mouseLeave(video);
  });
  expect(faded(container).shapes).toBe(0);
  expect(faded(container).slices.every(([, dim]) => dim === false)).toBe(true);
});

test("README 46.1, #383: moving to a legend row with the arrow keys previews it the same way", () => {
  const { container } = render(<TypeSlicesPreview scene="bar" />);
  const legend = screen.getByRole("listbox", { name: say("legendTitle") });
  const first = legend.querySelector("[role='option']") as HTMLElement;
  act(() => {
    fireEvent.keyDown(document.body, { key: "Tab" });
    first.focus();
  });
  // The first row is documents: the work folder's document slice stays bright.
  expect(faded(container).slices).toEqual([
    ["type.document", false],
    ["type.image", true],
    ["type.video", true],
  ]);
});
