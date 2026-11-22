import type { VizModel, VizRow } from "../ipc";
import { interpolate } from "../i18n/presentation";
import type { VisualizationMessageKey } from "./messages";

/** The label of the node that holds what no folder or type carries (#296). */
export const SANKEY_UNATTRIBUTED_LABEL = "ui.visualization.sankey.unattributed";
const TYPE_ROW = "group:viz.sankey.type.";
const UNATTRIBUTED_ROWS = new Set([
  "group:viz.sankey.unattributed",
  `${TYPE_ROW}unattributed`,
]);

type Words = {
  message: (
    key: VisualizationMessageKey,
    values?: Readonly<Record<string, string | bigint>>,
  ) => string;
  key: (key: string) => string;
  quantity: (value: NonNullable<VizRow["links"]>[number]["quantity"]) => string;
};

/**
 * The name of the row a Sankey link reaches: a type node by its legend class,
 * the unattributed node by its label. Links leave folder nodes for type nodes
 * only, so no other row is named here.
 */
export function sankeyLinkTarget(
  model: Pick<VizModel, "legend"> | undefined,
  toRowRef: string,
  text: Words & { className: (key: string) => string },
) {
  if (UNATTRIBUTED_ROWS.has(toRowRef))
    return text.key(SANKEY_UNATTRIBUTED_LABEL);
  if (model && toRowRef.startsWith(TYPE_ROW))
    return text.className(toRowRef.slice(TYPE_ROW.length));
  return text.message("itemNamePending");
}

/**
 * The links leaving a Sankey node row as words, in its order: "Projects to
 * Video, 80 GiB" (README 12.1). Empty for a row without links.
 */
export function sankeyLinkWords(
  model: Pick<VizModel, "legend"> | undefined,
  row: Pick<VizRow, "links">,
  from: string,
  text: Words & { className: (key: string) => string },
): string[] {
  return (row.links ?? []).map((link) => {
    const values: Record<string, string> = {
      from,
      to: sankeyLinkTarget(model, link.toRowRef, text),
      value: text.quantity(link.quantity),
    };
    // The values go with the key, as the product text fills them in; text
    // that only looks a key up gets them filled here.
    return withParticle(
      interpolate(
        text.message("sankeyLink", values),
        (name) => values[name] ?? `{${name}}`,
      ),
    );
  });
}

/**
 * Chooses the Korean "으로" or "로" after a word that ends in a Hangul syllable:
 * "로" after a vowel or ㄹ, "으로" after any other final consonant. After other
 * characters the written "(으)로" stays.
 */
export function withParticle(sentence: string) {
  return sentence.replace(/(.)\(으\)로/gu, (match, last: string) => {
    const code = last.codePointAt(0) ?? 0;
    if (code < 0xac00 || code > 0xd7a3) return match;
    const final = (code - 0xac00) % 28;
    return last + (final === 0 || final === 8 ? "로" : "으로");
  });
}
