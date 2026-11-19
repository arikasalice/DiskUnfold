import { useRef } from "react";
import type { ReactNode } from "react";
import { useLocaleText } from "../i18n/presentation";
import type { LocaleText } from "../i18n/presentation";
import { HintPopup, useHint } from "./hint";
import { NAME_SPACE_MARK, nameEdges, pathNameEdges } from "./name-edges";
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

/**
 * A path with a line break opportunity after each separator. A part that a
 * line could also break inside, at a space or a hyphen, is kept whole on a
 * line, so a line breaks after a separator first; only a part wider than the
 * line breaks inside it. Other parts stay plain text. The `<wbr>` elements
 * and the part spans hold no text of their own, so the text content and a
 * copy stay the path.
 */
function breakAfterSeparators(path: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  let start = 0;
  for (let index = 0; index < path.length; index += 1) {
    if (path[index] !== "/" && path[index] !== "\\") continue;
    nodes.push(
      pathPart(path.slice(start, index + 1), `part-${index}`),
      <wbr key={index} />,
    );
    start = index + 1;
  }
  nodes.push(pathPart(path.slice(start), "part-last"));
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

function pathPart(part: string, key: string): ReactNode {
  if (!INNER_BREAK.test(part)) return part;
  return (
    <span className="du-path__part" key={key}>
      {part}
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
 */
export function NameText({
  name,
  path = false,
}: {
  name: string;
  /** Treats `name` as a path and marks only its last component. */
  path?: boolean;
}) {
  const text = useLocaleText();
  const ref = useRef<HTMLSpanElement>(null);
  const parts = path
    ? pathNameEdges(name)
    : { head: "", name: nameEdges(name), tail: "" };
  const hints = edgeHints(parts.name, text);
  const hint = useHint(ref, hints.length > 0);
  if (hints.length === 0)
    return path ? (
      <span className="du-path">{breakAfterSeparators(name)}</span>
    ) : (
      name
    );
  const edges = parts.name;
  return (
    <span
      ref={ref}
      className={path ? "du-name du-path" : "du-name"}
      onPointerEnter={hint.enter}
      onPointerLeave={hint.leave}
    >
      {breakAfterSeparators(parts.head)}
      <span className="du-name__text" dir="auto">
        <Edge characters={edges.leading} count={edges.leadingCount} />
        {edges.middle}
        <Edge characters={edges.trailing} count={edges.trailingCount} />
      </span>
      {parts.tail}
      <span className="du-visually-hidden">{`, ${hints.join(", ")}`}</span>
      <HintPopup hint={hint} triggerRef={ref}>
        {hints.map((line) => (
          <span key={line}>{line}</span>
        ))}
      </HintPopup>
    </span>
  );
}
