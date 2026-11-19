import { Fragment, useRef } from "react";
import type { ReactNode } from "react";
import { useLocaleText } from "../i18n/presentation";
import type { LocaleText } from "../i18n/presentation";
import { HintPopup, useHint } from "./hint";
import {
  NAME_SPACE_MARK,
  invisibleCount,
  invisiblePieces,
  nameParts,
} from "./name-edges";
import type { NameEdges } from "./name-edges";
import "../theme/text-fit.css";
import "../theme/name-text.css";

function edgeHints(edges: NameEdges, text: LocaleText): string[] {
  const hints: string[] = [];
  if (edges.leadingCount > 0)
    hints.push(
      text(
        "ui.common.name_leading_spaces",
        edges.leadingCount === 1
          ? "The name starts with {count} space"
          : "The name starts with {count} spaces",
        { count: BigInt(edges.leadingCount) },
      ),
    );
  if (edges.trailingCount > 0)
    hints.push(
      text(
        "ui.common.name_trailing_spaces",
        edges.trailingCount === 1
          ? "The name ends with {count} space"
          : "The name ends with {count} spaces",
        { count: BigInt(edges.trailingCount) },
      ),
    );
  return hints;
}

/** The real edge white space, kept for copying, and the marks drawn in its place. */
function Edge({ characters, count }: { characters: string; count: number }) {
  if (count === 0) return null;
  return (
    <>
      <span className="du-name__space">{characters}</span>
      <span className="du-name__mark" aria-hidden="true">
        {NAME_SPACE_MARK.repeat(count)}
      </span>
    </>
  );
}

/** "U+200B" for a zero-width space. */
function codePoint(character: string): string {
  const hex = (character.codePointAt(0) ?? 0).toString(16).toUpperCase();
  return `U+${hex.padStart(4, "0")}`;
}

/**
 * Text with each invisible character kept for copying and its code point
 * drawn in its place. Text without one stays plain.
 */
function revealed(text: string): ReactNode {
  const pieces = invisiblePieces(text);
  if (!pieces.some((piece) => piece.invisible)) return text;
  return pieces.map((piece, index) =>
    piece.invisible ? (
      <span key={index}>
        <span className="du-name__hidden">{piece.text}</span>
        <span className="du-name__code" aria-hidden="true">
          {codePoint(piece.text)}
        </span>
      </span>
    ) : (
      piece.text
    ),
  );
}

/**
 * A path with a line break opportunity after each separator. A part that a
 * line could also break inside, at a space or a hyphen, is kept whole on a
 * line, so a line breaks after a separator first; only a part wider than the
 * line breaks inside it. Other parts stay plain text. The `<wbr>` elements
 * and the part spans hold no text of their own, so the text content and a
 * copy stay the path.
 */
function breakAfterSeparators(path: string, reveal = false): ReactNode[] {
  const nodes: ReactNode[] = [];
  let start = 0;
  for (let index = 0; index < path.length; index += 1) {
    if (path[index] !== "/" && path[index] !== "\\") continue;
    nodes.push(
      pathPart(path.slice(start, index + 1), `part-${index}`, reveal),
      <wbr key={index} />,
    );
    start = index + 1;
  }
  nodes.push(pathPart(path.slice(start), "part-last", reveal));
  return nodes;
}

/**
 * True for a value with a path separator, such as a scope root that names
 * itself with its whole path. Pass it as `path` to {@link NameText} where a
 * value can be either a name or a path.
 */
export function hasPathSeparator(value: string): boolean {
  return value.includes("/") || value.includes("\\");
}

/** Characters after which a line may break inside a path part. */
const INNER_BREAK = /[\s\u002D\u00AD\u2010-\u2015]/u;

function pathPart(part: string, key: string, reveal: boolean): ReactNode {
  const shown = reveal ? revealed(part) : part;
  if (!INNER_BREAK.test(part))
    return typeof shown === "string" ? (
      shown
    ) : (
      <Fragment key={key}>{shown}</Fragment>
    );
  return (
    <span className="du-path__part" key={key}>
      {shown}
    </span>
  );
}

/**
 * A file or folder name, or a path whose last component is a name, with its
 * edge white space made visible.
 *
 * Each white space character at the start or end of the name is drawn as a
 * muted "\u2423" at the size of the name. The marks are hidden from
 * assistive technology and from text selection, and the name keeps its real
 * characters, so the text content and a copied selection are the original
 * value. Pass the real name: search, sorting, comparison, copying and IPC
 * keep using that value, never the drawn marks.
 *
 * When the name has edge white space, an explanation such as "The name ends
 * with 1 space" follows the name for screen readers and appears beside it on
 * hover or when focus is on the item that holds the name. A name without edge
 * white space renders as the plain text it was before, with no element of its
 * own and no listener.
 *
 * With `path`, only the last component is treated as a name. The name takes
 * its own direction from its first strong character, so the marks of a
 * right-to-left name stand at its logical start and end.
 * A path also breaks after its separators before it breaks inside a
 * component, without changing its text.
 *
 * With `reveal`, for an expanded "Full name", every other invisible
 * character, such as a zero-width space, a direction mark or a control
 * character, is drawn in place as its code point ("U+200B") in a muted fixed
 * width font, and a line under the name gives their number. The real
 * characters stay in the text, so the text content and a copy are still the
 * original value, and their direction controls end where they stand, so a
 * name reversed by one shows its real order. Screen readers read the name
 * and then the same sentence, never the code points.
 */
export function NameText({
  name,
  path = false,
  reveal = false,
}: {
  name: string;
  /** Treats `name` as a path and marks only its last component. */
  path?: boolean;
  /** Draws the invisible characters too, for an expanded name. */
  reveal?: boolean;
}) {
  const text = useLocaleText();
  const ref = useRef<HTMLSpanElement>(null);
  const parts = nameParts(name, path);
  const hints = edgeHints(parts.name, text);
  const hint = useHint(ref, hints.length > 0);
  const invisible = reveal ? invisibleCount(name, path) : 0;
  const count =
    invisible === 0
      ? undefined
      : text(
          "ui.common.name_invisible_characters",
          invisible === 1
            ? "{count} invisible character"
            : "{count} invisible characters",
          { count: BigInt(invisible) },
        );
  if (hints.length === 0 && count === undefined)
    return path ? (
      <span className="du-path">{breakAfterSeparators(name)}</span>
    ) : (
      name
    );
  const edges = parts.name;
  const said = count === undefined ? hints : [...hints, count];
  return (
    <span
      ref={ref}
      className={path ? "du-name du-path" : "du-name"}
      onPointerEnter={hint.enter}
      onPointerLeave={hint.leave}
    >
      {breakAfterSeparators(parts.head, reveal)}
      <span className="du-name__text" dir="auto">
        <Edge characters={edges.leading} count={edges.leadingCount} />
        {reveal ? revealed(edges.middle) : edges.middle}
        <Edge characters={edges.trailing} count={edges.trailingCount} />
      </span>
      {parts.tail}
      <span className="du-visually-hidden">{`, ${said.join(", ")}`}</span>
      {count !== undefined && (
        <span className="du-name__count" aria-hidden="true">
          {count}
        </span>
      )}
      <HintPopup hint={hint} triggerRef={ref}>
        {hints.map((line) => (
          <span key={line}>{line}</span>
        ))}
      </HintPopup>
    </span>
  );
}
