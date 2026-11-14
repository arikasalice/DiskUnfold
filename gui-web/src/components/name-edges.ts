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
