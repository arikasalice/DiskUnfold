import { useEffect, useRef, useState } from "react";
import type { BrowseCopyPathOutcome, FolderQueryRow, ItemRef } from "../../ipc";
import { browseDisplayName } from "./browse-display";

export function useBrowseCopy(
  copyPath: ((item: ItemRef) => Promise<BrowseCopyPathOutcome>) | undefined,
  epoch: number,
  navigationSeq: () => number,
) {
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const generation = useRef(0);
  useEffect(() => {
    generation.current++;
    pending.current = false;
    setStatus("");
    setBusy(false);
    return () => {
      generation.current++;
    };
  }, [epoch]);
  function copy(row: FolderQueryRow) {
    if (!copyPath || pending.current) return;
    const request = generation.current;
    const isCurrent = () =>
      generation.current === request && navigationSeq() === epoch;
    pending.current = true;
    setBusy(true);
    const name = browseDisplayName(row.displayName);
    setStatus(`Copying path for ${name}…`);
    void copyPath(row.item)
      .then((result) => {
        if (!isCurrent()) return;
        if (result.outcome === "copied")
          setStatus(
            `Copied ${result.count} path${result.count === 1 ? "" : "s"} for ${name}.`,
          );
        else {
          const reasons = {
            "path.unrepresentable": "The path cannot be represented.",
            "path.multiline": "A multiline path cannot be copied.",
            "clipboard.unsupported": "The clipboard is not supported.",
            "clipboard.failed": "Could not copy to the clipboard.",
          };
          setStatus(
            `${name}: ${Object.hasOwn(reasons, result.reasonKey) ? reasons[result.reasonKey] : "Could not copy the path."}`,
          );
        }
      })
      .catch(() => {
        if (isCurrent()) setStatus(`Could not copy the path for ${name}.`);
      })
      .finally(() => {
        if (isCurrent()) {
          pending.current = false;
          setBusy(false);
        }
      });
  }
  return { copy, status, busy };
}
