import axe from "axe-core";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import {
  TitleBar,
  TitleBarProvider,
  useTitleBarContext,
} from "../src/components";
import {
  hasCustomTitlebar,
  requestWindowDrag,
  requestWindowZoom,
} from "../src/ipc";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  Reflect.deleteProperty(window, "webkit");
  Reflect.deleteProperty(window, "__diskunfoldTitlebar");
});

function Screen({ label }: { label: string }) {
  useTitleBarContext(label);
  return <button type="button">Inside the screen</button>;
}

function bar(enabled: boolean) {
  const drag = vi.fn();
  const zoom = vi.fn();
  const element = (
    <TitleBar
      icon="icon.svg"
      enabled={enabled}
      onDragStart={drag}
      onZoom={zoom}
    />
  );
  return { drag, zoom, element };
}

test("draws nothing where the native frame stays", () => {
  const { element } = bar(false);
  const { container } = render(<TitleBarProvider>{element}</TitleBarProvider>);
  expect(container.querySelector("[data-du-titlebar]")).toBeNull();
});

test("shows the app name and the current screen, and clears the screen on leave", () => {
  const { element } = bar(true);
  const view = render(
    <TitleBarProvider>
      {element}
      <Screen label="Startup and privacy" />
    </TitleBarProvider>,
  );
  const node = view.container.querySelector("[data-du-titlebar]");
  expect(node?.textContent).toContain("DiskUnfold");
  expect(node?.textContent).toContain("Startup and privacy");
  view.rerender(<TitleBarProvider>{element}</TitleBarProvider>);
  expect(node?.textContent).not.toContain("Startup and privacy");
});

test("pressing the empty bar drags the window and a double click zooms it", () => {
  const { element, drag, zoom } = bar(true);
  const { container } = render(<TitleBarProvider>{element}</TitleBarProvider>);
  const node = container.querySelector("[data-du-titlebar]") as HTMLElement;
  fireEvent.mouseDown(node, { button: 0, detail: 1 });
  expect(drag).toHaveBeenCalledTimes(1);
  fireEvent.mouseDown(node, { button: 2, detail: 1 });
  fireEvent.mouseDown(node, { button: 0, detail: 2 });
  expect(drag).toHaveBeenCalledTimes(1);
  fireEvent.doubleClick(node);
  expect(zoom).toHaveBeenCalledTimes(1);
});

test("a control inside the bar keeps its own behavior", () => {
  const { drag, zoom } = bar(true);
  render(<TitleBar icon="icon.svg" enabled onDragStart={drag} onZoom={zoom} />);
  const node = document.querySelector("[data-du-titlebar]") as HTMLElement;
  const control = document.createElement("button");
  node.append(control);
  fireEvent.mouseDown(control, { button: 0, detail: 1 });
  fireEvent.doubleClick(control);
  expect(drag).not.toHaveBeenCalled();
  expect(zoom).not.toHaveBeenCalled();
});

test("the bar is hidden from screen readers and axe finds no violation", async () => {
  const { element } = bar(true);
  const view = render(
    <TitleBarProvider>
      {element}
      <main>
        <button type="button">Content</button>
      </main>
    </TitleBarProvider>,
  );
  expect(
    view.container
      .querySelector("[data-du-titlebar]")
      ?.getAttribute("aria-hidden"),
  ).toBe("true");
  expect(screen.getByRole("button", { name: "Content" })).toBeTruthy();
  expect((await axe.run(view.container)).violations).toEqual([]);
});

test("the host marker and window message handler enable the bar and carry its messages", () => {
  const postMessage = vi.fn();
  expect(hasCustomTitlebar()).toBe(false);
  Object.defineProperty(window, "webkit", {
    configurable: true,
    value: { messageHandlers: { diskunfoldWindow: { postMessage } } },
  });
  Object.defineProperty(window, "__diskunfoldTitlebar", {
    configurable: true,
    value: true,
  });
  // jsdom serves the test page from another address, so the app document check refuses it.
  expect(hasCustomTitlebar()).toBe(false);
  requestWindowDrag();
  requestWindowZoom();
  expect(postMessage.mock.calls).toEqual([["drag"], ["zoom"]]);
});
