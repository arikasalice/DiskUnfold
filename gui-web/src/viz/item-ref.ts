import type { ItemRef } from "../ipc";

/** True when both references name the same item of the same result version. */
export function sameItem(
  a: ItemRef | null | undefined,
  b: ItemRef | null | undefined,
) {
  return (
    !!a && !!b && a.resultVersion === b.resultVersion && a.itemNo === b.itemNo
  );
}
