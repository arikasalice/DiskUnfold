import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { FileInspection } from "../src/features/analysis/FileInspection";
import type { BrowseDetail } from "../src/ipc";
import { sampleClient } from "./sample-support";

/*
 * #171, F02 README: a scope root names itself with its whole path. Its name
 * row then follows the path rule and breaks after a separator first; a plain
 * folder name stays plain text.
 * #176: "Full name" is offered only while the two shown lines cut the name or
 * the name holds characters that are not visible.
 * #178, F02 README: the expanded "Full name" draws each invisible character
 * as its code point and says how many there are; the copy, the text content
 * and the collapsed row keep the original name.
 */

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const root =
  "C:\\Users\\runneradmin\\AppData\\Local\\Temp\\diskunfold-smoke-24b69567\\scope";

function detail(name: string): BrowseDetail {
  return {
    item: { resultVersion: "rv", itemNo: 1 },
    displayPath: { state: "present", value: root },
    displayName: { state: "present", value: name },
    kind: { state: "present", value: "directory" },
    logicalSize: { state: "unknown", reasonKey: "ref.unavailable" },
    allocatedSize: { state: "unknown", reasonKey: "ref.unavailable" },
    createdAt: { state: "absent" },
    modifiedAt: { state: "absent" },
    observedAt: { state: "absent" },
    origin: "scan",
    completeness: "complete",
    currentCorrespondence: false,
    observationOnly: true,
  };
}

async function nameRow(name: string) {
  const fake = sampleClient({ "browse.detail": () => detail(name) });
  render(
    <FileInspection
      client={fake.client}
      item={{ resultVersion: "rv", itemNo: 1 }}
      itemKind="folder"
    />,
  );
  await screen.findByText("Scan record");
  return screen.getByText("Folder name").closest("div")!.querySelector("dd")!;
}

const fullName = () => screen.queryByRole("button", { name: /Full name/ });

/** Makes every element report more content than its box, as a cut clamp does. */
function cutEveryClamp() {
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(60);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(40);
}

function expectPath(path: Element) {
  expect(path.textContent).toBe(root);
  expect(path.querySelectorAll("wbr")).toHaveLength(7);
  expect(path.querySelector(".du-path__part")?.textContent).toBe(
    "diskunfold-smoke-24b69567\\",
  );
}

/** The text that is copied or read: the element without its drawn marks. */
function realText(element: Element): string {
  const copy = element.cloneNode(true) as Element;
  for (const drawn of copy.querySelectorAll("[aria-hidden='true']"))
    drawn.remove();
  return copy.textContent ?? "";
}

test("#171: a folder name that is a whole path breaks after its separators", async () => {
  const row = await nameRow(root);
  const paths = row.querySelectorAll(".du-path");
  expect(paths).toHaveLength(1);
  expectPath(paths[0]!);
  // #176: shown whole, so there is nothing more to see under "Full name".
  expect(fullName()).toBeNull();
});

test("#176: a cut name offers Full name, which follows the same path rule", async () => {
  cutEveryClamp();
  const row = await nameRow(root);
  fireEvent.click(fullName()!);
  const paths = row.querySelectorAll(".du-path");
  expect(paths).toHaveLength(2);
  for (const path of paths) expectPath(path);
});

test("#176: a name with a character that is not visible keeps Full name", async () => {
  await nameRow("scope\u200Bfolder");
  expect(fullName()).not.toBeNull();
});

test("#178: Full name draws invisible characters as code points and counts them", async () => {
  const name = " a\u200Bb\u202Ec ";
  const row = await nameRow(name);
  const collapsed = row.querySelector(".du-inspection-name > .du-text")!;
  expect(collapsed.querySelector(".du-name__code")).toBeNull();
  expect(collapsed.textContent).not.toContain("invisible");

  fireEvent.click(fullName()!);
  const expanded = row.querySelector(".du-inspection-name .du-text[tabindex]")!;
  const codes = expanded.querySelectorAll(".du-name__code");
  expect(Array.from(codes, (code) => code.textContent)).toEqual([
    "U+200B",
    "U+202E",
  ]);
  for (const code of codes)
    expect(code.getAttribute("aria-hidden")).toBe("true");
  // Edge spaces keep their own marks and are not counted again.
  expect(expanded.querySelectorAll(".du-name__mark")).toHaveLength(2);
  expect(realText(expanded.querySelector(".du-name__text")!)).toBe(name);
  // Read once after the name; the visible line is hidden from screen readers.
  expect(expanded.querySelector(".du-visually-hidden")?.textContent).toBe(
    ", The name starts with 1 space, The name ends with 1 space, 2 invisible characters",
  );
  const line = expanded.querySelector(".du-name__count")!;
  expect(line.textContent).toBe("2 invisible characters");
  expect(line.getAttribute("aria-hidden")).toBe("true");
});

test("#178: a whole path reveals invisible characters in any component", async () => {
  const path = "C:\\a\u200Db\\scope\u200E";
  const row = await nameRow(path);
  fireEvent.click(fullName()!);
  const expanded = row.querySelector(".du-inspection-name .du-text[tabindex]")!;
  expect(
    Array.from(
      expanded.querySelectorAll(".du-name__code"),
      (code) => code.textContent,
    ),
  ).toEqual(["U+200D", "U+200E"]);
  expect(expanded.querySelectorAll("wbr")).toHaveLength(2);
  expect(expanded.querySelector(".du-name__count")?.textContent).toBe(
    "2 invisible characters",
  );
});

test("#178: an emoji sequence draws its joiners, so it reveals nothing", async () => {
  await nameRow("family \u{1F468}\u200D\u{1F469}\u200D\u{1F467}");
  expect(fullName()).toBeNull();
});

test("#171: a plain folder name stays plain text and is shown whole", async () => {
  const row = await nameRow("scope");
  expect(row.querySelector(".du-path")).toBeNull();
  expect(row.querySelector("wbr")).toBeNull();
  expect(fullName()).toBeNull();
});
