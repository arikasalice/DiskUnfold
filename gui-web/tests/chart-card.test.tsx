import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { cardPlace } from "../src/components/charts/chart-card";
import { GeometryRenderer } from "../src/components/charts/geometry-renderer";
import { modelFixture } from "../preview/model-fixture";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { fixtureText } from "../preview/model-fixture";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function chart(note?: string) {
  return render(
    <GeometryRenderer
      modelRef="model-1"
      resultRevision="revision-1"
      label="Chart"
      selectedText="selected"
      fallbackNotice="Graph unavailable"
      missingFallbackNotice="Table unavailable"
      width={600}
      height={300}
      viewport={{ left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 }}
      selectedRowRef={null}
      onSelect={() => {}}
      fallback={{
        modelRef: "model-1",
        resultRevision: "revision-1",
        content: null,
      }}
      relations={[]}
      shapes={[
        {
          shapeRef: "clip",
          rowRef: "row-clip",
          accessibleLabel: "clip.mp4, 6 MiB, 26.1% of Current scope 23 MiB",
          ...(note ? { note } : {}),
          card: {
            title: "clip.mp4",
            lines: [
              "6 MiB (6291456 bytes)",
              "26.1% of Current scope 23 MiB",
              "Videos",
            ],
          },
          geometry: {
            primitive: "rect",
            points: [
              { x: 0, y: 0 },
              { x: 500_000, y: 1_000_000 },
            ],
          },
        },
      ]}
    />,
  );
}

const card = () => document.querySelector(".du-chart-card");
const lines = () =>
  Array.from(document.querySelectorAll(".du-chart-card p")).map(
    (line) => line.textContent,
  );

test("#323 README 8: focus opens the card at once, in order, hidden from assistive technology; Esc closes it", () => {
  chart();
  const shape = screen.getByRole("option");
  act(() => shape.focus());
  expect(lines()).toEqual([
    "clip.mp4",
    "6 MiB (6291456 bytes)",
    "26.1% of Current scope 23 MiB",
    "Videos",
  ]);
  expect(card()?.getAttribute("aria-hidden")).toBe("true");
  expect(card()?.querySelector("button, a, input")).toBeNull();
  fireEvent.keyDown(shape, { key: "Escape" });
  expect(card()).toBeNull();
});

test("#323 README 8: a pointer opens the card after resting 0.3 s and may move onto it", () => {
  vi.useFakeTimers();
  chart();
  const shape = screen.getByRole("option");
  fireEvent.pointerEnter(shape);
  act(() => vi.advanceTimersByTime(250));
  expect(card()).toBeNull();
  act(() => vi.advanceTimersByTime(60));
  expect(card()).not.toBeNull();
  fireEvent.pointerLeave(shape);
  fireEvent.pointerEnter(card()!);
  act(() => vi.advanceTimersByTime(500));
  expect(card()).not.toBeNull();
  fireEvent.pointerLeave(card()!);
  expect(card()).toBeNull();
});

test("#323 README 47: a bar drawn shorter says so on the line under its value", () => {
  chart("Drawn shorter than scale");
  act(() => screen.getByRole("option").focus());
  expect(lines().slice(1, 3)).toEqual([
    "6 MiB (6291456 bytes)",
    "Drawn shorter than scale",
  ]);
});

test("#323 README 22: the card sits above right, then above left, below right, below left, else in a fixed place", () => {
  const size = { offsetWidth: 200, offsetHeight: 80 };
  const view = { width: 1000, height: 700 };
  const box = (left: number, top: number, right: number, bottom: number) => ({
    left,
    top,
    right,
    bottom,
  });
  expect(cardPlace(box(100, 200, 300, 260), size, view)).toEqual({
    left: 308,
    top: 112,
    fixed: false,
  });
  // No room to the right: above left.
  expect(cardPlace(box(700, 200, 900, 260), size, view)).toMatchObject({
    left: 492,
    top: 112,
  });
  // No room above: below right.
  expect(cardPlace(box(100, 20, 300, 60), size, view)).toMatchObject({
    left: 308,
    top: 68,
  });
  // A shape filling the window leaves no side free: the fixed place.
  expect(cardPlace(box(0, 0, 1000, 700), size, view)).toEqual({
    left: 792,
    top: 612,
    fixed: true,
  });
});

test("#323 README 22: under a chart the folder's rows open folded", () => {
  const input = modelFixture();
  render(
    <VisualizationModelPresenter
      {...input}
      view={{
        viewport: { width: 960, height: 330, scale: 1 },
        visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
      }}
      selected={null}
      onSelect={vi.fn()}
      text={fixtureText}
    />,
  );
  const toggle = screen.getByRole("button", { name: /Items in this folder/ });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
});
