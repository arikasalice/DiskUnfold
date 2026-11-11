import { useEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import { rowKey } from "./browse-display";
import type { BrowseCopyPathOutcome, FolderQueryRow, ItemRef } from "../../ipc";
import { browseDisplayName } from "./browse-display";

export function useBrowseCopy(
  copyPath: ((item: ItemRef) => Promise<BrowseCopyPathOutcome>) | undefined,
  epoch: number,
  navigationSeq: () => number,
  scrollRoot: RefObject<HTMLDivElement | null>,
) {
  const [feedback, setFeedback] = useState<{
    beforeName: string;
    name: string;
    afterName: string;
  }>();
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const generation = useRef(0);
  useEffect(() => {
    generation.current++;
    pending.current = false;
    setFeedback(undefined);
    setBusy(false);
    return () => {
      generation.current++;
    };
  }, [epoch]);
  const origin = useRef<{ element: HTMLElement | null; key: string }>(
    undefined,
  );
  function dismiss(trigger: Element) {
    const restore = trigger.contains(document.activeElement);
    setFeedback(undefined);
    if (!restore) return;
    const row = origin.current;
    const target =
      row?.element?.isConnected && row.element.dataset.itemKey === row.key
        ? row.element
        : scrollRoot.current;
    target?.focus({ preventScroll: true });
  }
  function copy(row: FolderQueryRow, trigger: HTMLElement | null) {
    if (!copyPath || pending.current) return;
    origin.current = {
      element: trigger?.closest<HTMLElement>("[data-row]") ?? null,
      key: rowKey(row.item),
    };
    const request = generation.current;
    const requestNavigation = navigationSeq();
    const isCurrent = () =>
      generation.current === request && navigationSeq() === requestNavigation;
    pending.current = true;
    setBusy(true);
    const name = browseDisplayName(row.displayName);
    setFeedback({ beforeName: "Copying path for ", name, afterName: "…" });
    void copyPath(row.item)
      .then((result) => {
        if (!isCurrent()) return;
        if (result.outcome === "copied")
          setFeedback({
            beforeName: `Copied ${result.count} path${result.count === 1 ? "" : "s"} for `,
            name,
            afterName: ".",
          });
        else {
          const reasons = {
            "path.unrepresentable": "The path cannot be represented.",
            "path.multiline": "A multiline path cannot be copied.",
            "clipboard.unsupported": "The clipboard is not supported.",
            "clipboard.failed": "Could not copy to the clipboard.",
          };
          setFeedback({
            beforeName: "",
            name,
            afterName: `: ${Object.hasOwn(reasons, result.reasonKey) ? reasons[result.reasonKey] : "Could not copy the path."}`,
          });
        }
      })
      .catch(() => {
        if (isCurrent())
          setFeedback({
            beforeName: "Could not copy the path for ",
            name,
            afterName: ".",
          });
      })
      .finally(() => {
        if (generation.current === request) {
          pending.current = false;
          setBusy(false);
          if (!isCurrent()) setFeedback(undefined);
        }
      });
  }
  const status = feedback
    ? feedback.beforeName + feedback.name + feedback.afterName
    : "";
  return { copy, dismiss, status, feedback, busy };
}
