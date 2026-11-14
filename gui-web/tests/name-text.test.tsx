import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { Action, Breadcrumbs, NameText, Text } from "../src/components";

afterEach(cleanup);

function marks(container: ParentNode) {
  return Array.from(container.querySelectorAll(".du-name__mark"));
}

test("a name without edge white space is plain text", () => {
  const { container } = render(<NameText name="보고서 초안" />);
  expect(container.textContent).toBe("보고서 초안");
  expect(marks(container)).toHaveLength(0);
  expect(container.querySelector(".du-visually-hidden")).toBeNull();
});

test("edge white space is drawn as hidden marks and the text keeps the real name", () => {
  const { container } = render(<NameText name={"  초안\t"} />);
  const shown = marks(container);
  expect(shown.map((mark) => mark.textContent)).toEqual([
    "\u2423\u2423",
    "\u2423",
  ]);
  for (const mark of shown)
    expect(mark.getAttribute("aria-hidden")).toBe("true");
  const text = container.querySelector(".du-name__text")!;
  expect(text.getAttribute("dir")).toBe("auto");
  const real = Array.from(text.childNodes)
    .filter(
      (node) => !(node instanceof Element && node.matches(".du-name__mark")),
    )
    .map((node) => node.textContent)
    .join("");
  expect(real).toBe("  초안\t");
});

test("screen readers hear the name followed by the explanation, in the right number", () => {
  render(
    <>
      <Action onPress={() => undefined}>
        <NameText name="보고서 " />
      </Action>
      <Action onPress={() => undefined}>
        <NameText name={"  초안  "} />
      </Action>
    </>,
  );
  expect(
    screen.getByRole("button", {
      name: /^보고서\s*, The name ends with 1 space$/,
    }),
  ).toBeDefined();
  expect(
    screen.getByRole("button", {
      name: /^\s*초안\s*, The name starts with 2 spaces, The name ends with 2 spaces$/,
    }),
  ).toBeDefined();
});

test("the explanation shows on hover and on focus of the item, and Escape hides it", async () => {
  render(
    <section tabIndex={0} aria-label="Region">
      <Action onPress={() => undefined}>
        <NameText name="보고서 " />
      </Action>
    </section>,
  );
  const region = screen.getByRole("region", { name: "Region" });
  act(() => region.focus());
  expect(screen.queryByRole("tooltip")).toBeNull();

  const button = screen.getByRole("button");
  act(() => button.focus());
  expect((await screen.findByRole("tooltip")).textContent).toBe(
    "The name ends with 1 space",
  );
  fireEvent.keyDown(button, { key: "Escape" });
  expect(screen.queryByRole("tooltip")).toBeNull();
  act(() => button.blur());

  fireEvent.pointerEnter(document.querySelector(".du-name")!);
  expect(await screen.findByRole("tooltip")).toBeDefined();
  fireEvent.pointerLeave(document.querySelector(".du-name")!);
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("a path marks only its last component", () => {
  const { container } = render(
    <Text kind="path">
      <NameText path name={"C:\\ 민서\\보고서 "} />
    </Text>,
  );
  expect(marks(container).map((mark) => mark.textContent)).toEqual(["\u2423"]);
  expect(container.querySelector(".du-name__text")?.textContent).toBe(
    "보고서 \u2423",
  );
});

test("the current location of a path line is drawn as a name", () => {
  const { container } = render(
    <Breadcrumbs
      label="Folder path"
      entries={[
        { id: "a", label: " 상위", onPress: () => undefined },
        { id: "b", label: "보고서 ", current: true },
      ]}
    />,
  );
  const current = container.querySelector('[aria-current="location"]')!;
  expect(current.getAttribute("title")).toBe("보고서 ");
  expect(marks(current)).toHaveLength(1);
  expect(marks(container)).toHaveLength(1);
});
