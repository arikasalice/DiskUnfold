import { expect, test } from "vitest";
import { drawnParts, fitLine } from "../src/components/line-fit";
import { splitForMiddleEllipsis } from "../src/components/text-fit";

/** Every character is one unit wide, so a room is a number of characters. */
const width = (value: string) => Array.from(value).length;

/** The line `text` draws in `room` characters, with its "…" marks. */
function line(text: string, room: number) {
  const { head, tail } = splitForMiddleEllipsis(text);
  const drawn = drawnParts(head, tail, fitLine(width, room, text, head, tail));
  return (
    (drawn.headHidden ? "" : drawn.head) +
    (drawn.headCut || drawn.headHidden ? "\u2026" : "") +
    drawn.name +
    (drawn.nameCut ? "\u2026" : "") +
    drawn.end
  );
}

const path = "C:\\Users\\minseo\\a\\b\\report.docx";

test("a path that fits is drawn whole, with no mark", () => {
  expect(line(path, path.length)).toBe(path);
});

test("a cut head keeps what fits and its mark meets the tail", () => {
  expect(line(path, 30)).toBe("C:\\Use\u2026\\minseo\\a\\b\\report.docx");
});

test("a hidden head starts the line with a mark and keeps the parent folders that fit", () => {
  expect(line(path, 20)).toBe("\u2026\\a\\b\\report.docx");
  expect(line(path, 14)).toBe("\u2026\\report.docx");
});

test("a last component wider than the line keeps its extension with no gap", () => {
  expect(line(path, 10)).toBe("\u2026\\re\u2026.docx");
  expect(line("D:\\photos\\family trip all.zip", 17)).toBe(
    "\u2026\\family tri\u2026.zip",
  );
});
