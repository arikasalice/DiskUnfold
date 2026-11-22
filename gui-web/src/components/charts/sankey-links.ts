/**
 * The parts of a Sankey chart the shared geometry renderer needs (F06 README
 * 12.1): moving between and within its columns, and which links a node or link
 * under the pointer or focus keeps in view.
 */

/** The columns of a Sankey, left to right. */
export const SANKEY_COLUMNS = ["scope", "folder", "type"] as const;

type Box = { left: number; top: number; right: number; bottom: number };
type Placed = { column?: string | undefined; box: Box };

const middle = (box: Box) => (box.top + box.bottom) / 2;

/**
 * The node an arrow key moves to in a Sankey: up and down to the node above or
 * below in the same column, left and right to the node of the next column whose
 * middle is nearest (the upper one of two as near). At an edge there is none.
 */
export function columnStep(
  items: readonly Placed[],
  from: number,
  key: string,
): number | undefined {
  const origin = items[from];
  const column = SANKEY_COLUMNS.findIndex((name) => name === origin?.column);
  if (!origin || column < 0) return undefined;
  const inColumn = (name: string | undefined) =>
    items
      .map((item, index) => ({ item, index }))
      .filter(({ item }) => item.column === name)
      .sort((a, b) => a.item.box.top - b.item.box.top);
  if (key === "ArrowUp" || key === "ArrowDown") {
    const same = inColumn(origin.column);
    const at = same.findIndex(({ index }) => index === from);
    return same[at + (key === "ArrowDown" ? 1 : -1)]?.index;
  }
  const next = SANKEY_COLUMNS[column + (key === "ArrowRight" ? 1 : -1)];
  if (!next) return undefined;
  const centre = middle(origin.box);
  let best: { index: number; distance: number } | undefined;
  for (const { item, index } of inColumn(next)) {
    const distance = Math.abs(middle(item.box) - centre);
    if (!best || distance < best.distance) best = { index, distance };
  }
  return best?.index;
}

/** What the pointer or focus is on: a node by its shape reference, or a link by its place. */
export type LinkFocus = { shapeRef: string } | { link: number } | null;

/**
 * The links that stay in view while a node or link is pointed at or focused:
 * those that touch the node, or the link itself. Null when nothing is.
 */
export function keptLinks(
  relations: readonly { fromShapeRef: string; toShapeRef: string }[],
  focus: LinkFocus,
): ReadonlySet<number> | null {
  if (!focus) return null;
  if ("link" in focus) return new Set([focus.link]);
  const kept = new Set<number>();
  relations.forEach((relation, index) => {
    if (
      relation.fromShapeRef === focus.shapeRef ||
      relation.toShapeRef === focus.shapeRef
    )
      kept.add(index);
  });
  return kept;
}
