import axe from "axe-core";
import { readFileSync } from "node:fs";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { ClampedText, MiddleEllipsisText } from "../src/components";
import { splitForMiddleEllipsis } from "../src/components/text-fit";

afterEach(() => {
  cleanup();
  delete (HTMLElement.prototype as { scrollHeight?: number }).scrollHeight;
  delete (HTMLElement.prototype as { clientHeight?: number }).clientHeight;
});

/** Two long messages per language, as the F13 long-text review set uses. */
const LONG_TEXT = [
  [
    "en",
    "DiskUnfold could not finish reading this folder because several items changed while it was being observed, so the totals shown here may be lower than the real size.",
  ],
  [
    "en",
    "Review every file in this cleanup plan before you approve it; nothing is removed until you confirm each group in the next step.",
  ],
  [
    "ko",
    "이 폴더를 읽는 동안 여러 항목이 바뀌어서 읽기를 끝내지 못했습니다. 여기에 보이는 합계는 실제 크기보다 작을 수 있으니 다시 확인해 주세요.",
  ],
  [
    "ko",
    "정리 계획의 모든 파일을 승인하기 전에 검토해 주세요. 다음 단계에서 각 묶음을 확인하기 전까지는 아무것도 지워지지 않습니다.",
  ],
] as const;

const PATHS = [
  "C:\\Users\\someone\\Documents\\Projects\\2026\\quarterly-report-final-reviewed-v12.xlsx",
  "/Users/someone/사진/가족 여행/2026년 여름 휴가/제주도 해변 일출 사진 모음 원본.tar.gz",
];

/** happy-dom does not lay out text, so the box and content heights are supplied. */
function overflowing(on: boolean) {
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get: () => 40,
  });
  Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
    configurable: true,
    get: () => (on ? 120 : 40),
  });
}

test.each(LONG_TEXT)(
  "%s long text that fits keeps its full text and does not take focus",
  (_language, text) => {
    overflowing(false);
    const { container } = render(<ClampedText text={text} />);
    expect(screen.getByText(text).className).toBe("du-clamp");
    expect(container.querySelector("[tabindex]")).toBeNull();
    expect(container.querySelector(".du-fit__full")).toBeNull();
  },
);

test.each(LONG_TEXT)(
  "%s long text cut by the two line limit takes focus and shows itself in full",
  async (_language, text) => {
    overflowing(true);
    const { container } = render(<ClampedText text={text} lines={2} />);
    await act(async () => {});
    const frame = container.querySelector(".du-fit") as HTMLElement;
    expect(frame.getAttribute("tabindex")).toBe("0");
    const clamp = container.querySelector(".du-clamp") as HTMLElement;
    expect(clamp.textContent).toBe(text);
    expect(clamp.style.getPropertyValue("--du-clamp-lines")).toBe("2");
    const full = container.querySelector(".du-fit__full") as HTMLElement;
    expect(full.textContent).toBe(text);
    // The visual aid is not read a second time next to the content.
    expect(full.getAttribute("aria-hidden")).toBe("true");
    expect((await axe.run(container)).violations).toEqual([]);
  },
);

test("Escape hides the full text of a cut text until focus leaves", async () => {
  overflowing(true);
  const { container } = render(<ClampedText text={LONG_TEXT[0][1]} />);
  await act(async () => {});
  const frame = container.querySelector(".du-fit") as HTMLElement;
  fireEvent.keyDown(frame, { key: "Escape" });
  expect(frame.hasAttribute("data-dismissed")).toBe(true);
  fireEvent.blur(frame);
  expect(frame.hasAttribute("data-dismissed")).toBe(false);
});

test("the clamp is two lines by default and the text fit styles set no fixed width", () => {
  const css = readFileSync("src/theme/text-fit.css", "utf8");
  expect(css).toMatch(/-webkit-line-clamp: var\(--du-clamp-lines, 2\)/);
  for (const selector of [".du-fit", ".du-clamp", ".du-middle"]) {
    const rule =
      css.match(new RegExp(`^\\${selector} \\{[^}]*\\}`, "m"))?.[0] ?? "";
    expect(rule).not.toBe("");
    expect(rule).not.toMatch(
      /(?:^|\s)(?:inline-size|width|min-width|max-width): \d+px/,
    );
  }
});

test.each(PATHS)(
  "the middle ellipsis keeps the original path as the accessible name: %s",
  async (path) => {
    overflowing(false);
    const { container } = render(<MiddleEllipsisText text={path} />);
    const shown = Array.from(
      container.querySelectorAll('.du-middle [aria-hidden="true"]'),
    )
      .map((node) => node.textContent)
      .join("");
    expect(shown).toBe(path);
    expect(container.querySelector(".du-visually-hidden")?.textContent).toBe(
      path,
    );
    expect((await axe.run(container)).violations).toEqual([]);
  },
);

test("a cut path takes focus and shows the original path in full", async () => {
  overflowing(true);
  const path = PATHS[0]!;
  const { container } = render(<MiddleEllipsisText text={path} />);
  await act(async () => {});
  expect(
    (container.querySelector(".du-fit") as HTMLElement).getAttribute(
      "tabindex",
    ),
  ).toBe("0");
  expect(container.querySelector(".du-fit__full")?.textContent).toBe(path);
});

test("the tail keeps the extension and short names are not split", () => {
  expect(splitForMiddleEllipsis("report.xlsx")).toEqual({
    head: "report.xlsx",
    tail: "",
  });
  const { head, tail } = splitForMiddleEllipsis(PATHS[0]!);
  expect(head + tail).toBe(PATHS[0]);
  expect(tail.endsWith(".xlsx")).toBe(true);
  const korean = splitForMiddleEllipsis(PATHS[1]!);
  expect(korean.head + korean.tail).toBe(PATHS[1]);
  expect(korean.tail.endsWith(".tar.gz")).toBe(true);
});
