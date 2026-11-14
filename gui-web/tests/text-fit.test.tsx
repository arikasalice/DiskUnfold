import axe from "axe-core";
import { readFileSync } from "node:fs";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
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

/** Every offset at which a user-perceived character begins or ends. */
function graphemeBoundaries(text: string): Set<number> {
  const offsets = new Set<number>([text.length]);
  const segmenter = new Intl.Segmenter("und", { granularity: "grapheme" });
  for (const part of segmenter.segment(text)) offsets.add(part.index);
  return offsets;
}

function expectCleanSplit(text: string) {
  const { head, tail } = splitForMiddleEllipsis(text);
  expect(head + tail).toBe(text);
  expect(graphemeBoundaries(text).has(head.length)).toBe(true);
  return { head, tail };
}

const NFD_NAME = "\u1112\u1161\u11AB\u1100\u1173\u11AF.txt";
const FAMILY = "\u{1F468}\u200D\u{1F469}\u200D\u{1F467}";

test.each([
  ["Windows", "C:\\Users\\someone\\Documents\\Projects\\2026\\", "\\"],
  ["POSIX", "/home/someone/Documents/Projects/2026/", "/"],
])(
  "%s path: the whole file name stays in the tail, after its separator",
  (_system, folder, separator) => {
    for (const name of [
      NFD_NAME,
      "e\u0301-report.txt",
      `${FAMILY}.png`,
      "\u{20BB7}\u{20BB7}.md",
      "\u0645\u0644\u0641-\u0627\u0644\u062A\u0642\u0631\u064A\u0631.pdf",
      "\u05D3\u05D5\u05D7-\u05E9\u05E0\u05EA\u05D9.docx",
    ]) {
      const { head, tail } = expectCleanSplit(folder + name);
      expect(tail.endsWith(name)).toBe(true);
      expect(tail.startsWith(separator)).toBe(true);
      expect(head.startsWith(folder.slice(0, 3))).toBe(true);
    }
  },
);

test("a short parent folder stays with the file name and a long one does not", () => {
  const near = expectCleanSplit(
    "/Users/someone/Documents/DiskUnfold/snapshots",
  );
  expect(near.tail).toBe("/DiskUnfold/snapshots");
  const far = expectCleanSplit(
    "C:\\Users\\someone\\AppData\\Local\\DiskUnfold\\data\\workspace-01\\snapshots",
  );
  expect(far.tail).toBe("\\workspace-01\\snapshots");
  expect(far.head.startsWith("C:\\Users")).toBe(true);
});

test("a trailing separator, mixed separators and a root path are handled", () => {
  expect(
    expectCleanSplit("/var/lib/some/deep/folder/location/archive/").tail,
  ).toBe("/location/archive/");
  expect(expectCleanSplit("C:\\data\\a/b\\c/some-long-folder-name").tail).toBe(
    "\\c/some-long-folder-name",
  );
  const root = expectCleanSplit("/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
  expect(root.head.length).toBeGreaterThan(0);
});

test("a last component longer than the tail limit is cut between whole characters", () => {
  for (const unit of [
    "\u1112\u1161\u11AB",
    "e\u0301",
    FAMILY,
    "\u{20BB7}",
    "\u05D3",
  ]) {
    const name = unit.repeat(30) + ".tar.gz";
    for (const text of [name, "/Users/someone/" + name, "C:\\x\\" + name]) {
      const { tail } = expectCleanSplit(text);
      expect(tail.endsWith(".tar.gz")).toBe(true);
      expect(Array.from(tail).length).toBeGreaterThan(1);
    }
  }
});

test("text of sixteen characters or fewer is not split, even with combining marks", () => {
  expect(splitForMiddleEllipsis("e\u0301".repeat(16))).toEqual({
    head: "e\u0301".repeat(16),
    tail: "",
  });
});

test("the visible parts keep the direction of the whole text and isolate each other", () => {
  const arabic =
    "/home/someone/documents/\u0645\u0644\u0641-\u0627\u0644\u062A\u0642\u0631\u064A\u0631.pdf";
  const { container } = render(<MiddleEllipsisText text={arabic} />);
  expect(container.querySelector(".du-middle")?.getAttribute("dir")).toBe(
    "auto",
  );
  expect(
    container.querySelector(".du-visually-hidden")?.getAttribute("dir"),
  ).toBe("auto");
  const shown = Array.from(
    container.querySelectorAll('.du-middle [aria-hidden="true"]'),
  )
    .map((node) => node.textContent)
    .join("");
  expect(shown).toBe(arabic);
  const css = readFileSync("src/theme/text-fit.css", "utf8");
  expect(css).toMatch(
    /\.du-middle__head,\n\.du-middle__tail \{[^}]*unicode-bidi: isolate/,
  );
});

test("neither part of a path can push the line wider than its container", () => {
  const css = readFileSync("src/theme/text-fit.css", "utf8");
  const shared =
    css.match(/\.du-middle__head,\n\.du-middle__tail \{[^}]*\}/)?.[0] ?? "";
  expect(shared).toMatch(/min-inline-size: 0/);
  expect(shared).toMatch(/overflow: hidden/);
  expect(shared).toMatch(/text-overflow: ellipsis/);
  const tail =
    [...css.matchAll(/^\.du-middle__tail \{[^}]*\}/gm)].at(-1)?.[0] ?? "";
  expect(tail).toMatch(/max-inline-size: 100%/);
});

test("Korean prose breaks between words and long paths still wrap", () => {
  const css = readFileSync("src/theme/text-fit.css", "utf8");
  const rule = css.match(/^\.du-clamp \{[^}]*\}/m)?.[0] ?? "";
  expect(rule).toMatch(/word-break: keep-all/);
  expect(rule).toMatch(/overflow-wrap: anywhere/);
});

test("text that is not cut has no tab stop, role or full-text aid", () => {
  overflowing(false);
  const { container } = render(<MiddleEllipsisText text={PATHS[0]!} />);
  expect(container.querySelector("[tabindex]")).toBeNull();
  expect(container.querySelector("[role]")).toBeNull();
  expect(container.querySelector(".du-fit__full")).toBeNull();
});

test("a cut text is a focusable note whose content is its full text", async () => {
  overflowing(true);
  const { container } = render(<MiddleEllipsisText text={PATHS[0]!} />);
  await act(async () => {});
  const frame = container.querySelector(".du-fit") as HTMLElement;
  expect(frame.getAttribute("role")).toBe("note");
  expect(frame.getAttribute("tabindex")).toBe("0");
  expect((await axe.run(container)).violations).toEqual([]);
});

test("a pointer-opened full text closes with Escape and returns on the next hover", async () => {
  overflowing(true);
  const outside = vi.fn();
  document.body.addEventListener("keydown", outside);
  const { container } = render(<MiddleEllipsisText text={PATHS[0]!} />);
  await act(async () => {});
  const frame = container.querySelector(".du-fit") as HTMLElement;
  fireEvent.mouseEnter(frame);
  // Focus is elsewhere: the key goes to the body, not to the frame.
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(frame.hasAttribute("data-dismissed")).toBe(true);
  expect(outside).not.toHaveBeenCalled();
  fireEvent.mouseLeave(frame);
  expect(frame.hasAttribute("data-dismissed")).toBe(false);
  fireEvent.mouseEnter(frame);
  expect(frame.hasAttribute("data-dismissed")).toBe(false);
  document.body.removeEventListener("keydown", outside);
});

test("Escape that the input method is processing leaves the full text open", async () => {
  overflowing(true);
  const { container } = render(<ClampedText text={LONG_TEXT[0][1]} />);
  await act(async () => {});
  const frame = container.querySelector(".du-fit") as HTMLElement;
  fireEvent.mouseEnter(frame);
  fireEvent.keyDown(document.body, { key: "Escape", keyCode: 229 });
  expect(frame.hasAttribute("data-dismissed")).toBe(false);
});

test("copying a selection inside a cut path puts the full path on the clipboard once", async () => {
  overflowing(true);
  const path = PATHS[0]!;
  const { container } = render(<MiddleEllipsisText text={path} />);
  await act(async () => {});
  const frame = container.querySelector(".du-fit") as HTMLElement;
  window.getSelection()?.selectAllChildren(frame);
  const setData = vi.fn();
  const notCancelled = fireEvent.copy(frame, { clipboardData: { setData } });
  expect(setData).toHaveBeenCalledTimes(1);
  expect(setData).toHaveBeenCalledWith("text/plain", path);
  expect(notCancelled).toBe(false);
  const css = readFileSync("src/theme/text-fit.css", "utf8");
  expect(css).toMatch(
    /\.du-fit \.du-visually-hidden,\n\.du-fit__full \{[^}]*user-select: none/,
  );
});

test("a selection that is collapsed or outside the path is copied as the browser does", async () => {
  overflowing(true);
  const { container } = render(
    <div>
      <p id="other">other</p>
      <MiddleEllipsisText text={PATHS[0]!} />
    </div>,
  );
  await act(async () => {});
  const frame = container.querySelector(".du-fit") as HTMLElement;
  const setData = vi.fn();
  window.getSelection()?.removeAllRanges();
  fireEvent.copy(frame, { clipboardData: { setData } });
  window.getSelection()?.selectAllChildren(container.querySelector("#other")!);
  fireEvent.copy(frame, { clipboardData: { setData } });
  expect(setData).not.toHaveBeenCalled();
});
