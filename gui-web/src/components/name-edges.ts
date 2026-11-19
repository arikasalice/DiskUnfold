import { graphemes } from "./graphemes";

/** The mark drawn for each white space character at either end of a name. */
export const NAME_SPACE_MARK = "\u2423";

/**
 * A name split at its edge white space. `leading + middle + trailing` is
 * always the original name, and the counts are code points, one per white
 * space character.
 */
export type NameEdges = {
  leading: string;
  middle: string;
  trailing: string;
  leadingCount: number;
  trailingCount: number;
};

/**
 * A path split around its last component: `head` ends with the separator
 * before it, `name` is that component split at its edges and `tail` holds
 * any separators after it. The three parts join back into the path.
 */
export type PathNameEdges = { head: string; name: NameEdges; tail: string };

const WHITE_SPACE = /^\p{White_Space}+$/u;

/**
 * Characters that take no visible space: format and control characters, such
 * as a zero-width space or a direction mark, and the Hangul fillers.
 */
const INVISIBLE_CHARACTER = /[\p{Cf}\p{Cc}\u115F\u1160\u3164\uFFA0]/u;

const PICTOGRAPHIC = /\p{Extended_Pictographic}/u;

/** A run of visible text, or one invisible character. */
export type NamePiece = { text: string; invisible: boolean };

/**
 * Splits text at its invisible characters, one piece for each. A joiner or a
 * tag inside an emoji sequence is part of the emoji that is drawn, so it is
 * not invisible.
 */
export function invisiblePieces(text: string): NamePiece[] {
  const pieces: NamePiece[] = [];
  let run = "";
  for (const unit of graphemes(text)) {
    if (PICTOGRAPHIC.test(unit)) {
      run += unit;
      continue;
    }
    for (const character of unit) {
      if (!INVISIBLE_CHARACTER.test(character)) {
        run += character;
        continue;
      }
      if (run !== "") pieces.push({ text: run, invisible: false });
      pieces.push({ text: character, invisible: true });
      run = "";
    }
  }
  if (run !== "") pieces.push({ text: run, invisible: false });
  return pieces;
}

/**
 * Only a character that is white space on its own counts. A space that
 * carries a combining mark is drawn as that mark, so it is not blank.
 */
function blank(unit: string | undefined): boolean {
  return unit !== undefined && WHITE_SPACE.test(unit);
}

/**
 * Finds the white space at the start and the end of a name.
 *
 * White space is the Unicode `White_Space` property: space, tab, line breaks,
 * no-break space and the other space characters. White space inside the name
 * stays in `middle`. A name made only of white space is all `leading`, so
 * every character is marked once. The split follows grapheme clusters, so it
 * never separates a character from its combining marks.
 */
export function nameEdges(name: string): NameEdges {
  const units = graphemes(name);
  let start = 0;
  while (blank(units[start])) start += 1;
  let end = units.length;
  while (end > start && blank(units[end - 1])) end -= 1;
  const leading = units.slice(0, start).join("");
  const trailing = units.slice(end).join("");
  return {
    leading,
    middle: units.slice(start, end).join(""),
    trailing,
    leadingCount: Array.from(leading).length,
    trailingCount: Array.from(trailing).length,
  };
}

function isSeparator(character: string | undefined): boolean {
  return character === "/" || character === "\\";
}

/**
 * Finds the edge white space of a path's last component. Both `/` and `\\`
 * separate components, and separators at the end of the path are not part of
 * the last component.
 */
export function pathNameEdges(path: string): PathNameEdges {
  let end = path.length;
  while (end > 0 && isSeparator(path[end - 1])) end -= 1;
  let start = end;
  while (start > 0 && !isSeparator(path[start - 1])) start -= 1;
  return {
    head: path.slice(0, start),
    name: nameEdges(path.slice(start, end)),
    tail: path.slice(end),
  };
}

/**
 * A name, or with `path` a path, split around the name whose edge white space
 * is marked.
 */
export function nameParts(value: string, path: boolean): PathNameEdges {
  return path
    ? pathNameEdges(value)
    : { head: "", name: nameEdges(value), tail: "" };
}

/**
 * The number of invisible characters an expanded name draws a marker for:
 * every one in the value except the edge white space of the name, which is
 * drawn as "\u2423".
 */
export function invisibleCount(value: string, path = false): number {
  const parts = nameParts(value, path);
  return [parts.head, parts.name.middle, parts.tail]
    .flatMap(invisiblePieces)
    .filter((piece) => piece.invisible).length;
}
