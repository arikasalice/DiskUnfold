import { useEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import { rowKey } from "./browse-display";
import type { BrowseCopyPathOutcome, FolderQueryRow, ItemRef } from "../../ipc";
import { browseDisplayName } from "./browse-display";
import { useLocaleText } from "../../i18n/presentation";

type LocaleText = ReturnType<typeof useLocaleText>;
type CopyPathRefusal = Extract<
  BrowseCopyPathOutcome,
  { outcome: "refused" }
>["reasonKey"];

type CopyFeedback =
  | { kind: "copying" | "failed"; name: string }
  | { kind: "copied"; name: string; count: number }
  | {
      kind: "refused";
      name: string;
      reasonKey: CopyPathRefusal;
    };

export function useBrowseCopy(
  copyPath: ((item: ItemRef) => Promise<BrowseCopyPathOutcome>) | undefined,
  epoch: number,
  navigationSeq: () => number,
  scrollRoot: RefObject<HTMLDivElement | null>,
) {
  const text = useLocaleText();
  const [feedbackState, setFeedback] = useState<CopyFeedback>();
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
    const name = browseDisplayName(row.displayName, text);
    setFeedback({ kind: "copying", name });
    void copyPath(row.item)
      .then((result) => {
        if (!isCurrent()) return;
        if (result.outcome === "copied") {
          setFeedback({ kind: "copied", name, count: result.count });
        } else {
          setFeedback({ kind: "refused", name, reasonKey: result.reasonKey });
        }
      })
      .catch(() => {
        if (isCurrent()) setFeedback({ kind: "failed", name });
      })
      .finally(() => {
        if (generation.current === request) {
          pending.current = false;
          setBusy(false);
          if (!isCurrent()) setFeedback(undefined);
        }
      });
  }
  const feedback = feedbackState
    ? splitName(feedbackMessage(feedbackState, text), feedbackState.name)
    : undefined;
  const status = feedback
    ? feedback.beforeName + feedback.name + feedback.afterName
    : "";
  return { copy, dismiss, status, feedback, busy };
}

function feedbackMessage(feedback: CopyFeedback, text: LocaleText) {
  switch (feedback.kind) {
    case "copying":
      return text("ui.browse.copying_path_for", "Copying path for {name}…", {
        name: feedback.name,
      });
    case "copied":
      return feedback.count === 1
        ? text("ui.browse.copied_one_path_for", "Copied one path for {name}.", {
            name: feedback.name,
          })
        : text(
            "ui.browse.copied_many_paths_for",
            "Copied {count} paths for {name}.",
            { count: String(feedback.count), name: feedback.name },
          );
    case "refused":
      switch (feedback.reasonKey) {
        case "path.unrepresentable":
          return text(
            "ui.browse.path_unrepresentable_for",
            "Could not copy the path for {name}: the path cannot be represented.",
            { name: feedback.name },
          );
        case "path.multiline":
          return text(
            "ui.browse.path_multiline_for",
            "Could not copy the path for {name}: multiline paths cannot be copied.",
            { name: feedback.name },
          );
        case "clipboard.unsupported":
          return text(
            "ui.browse.clipboard_unsupported_for",
            "Could not copy the path for {name}: clipboard support is unavailable.",
            { name: feedback.name },
          );
        case "clipboard.failed":
          return text(
            "ui.browse.clipboard_failed_for",
            "Could not copy the path for {name}: clipboard copy failed.",
            { name: feedback.name },
          );
        default:
          return text(
            "ui.browse.copy_path_failed_for",
            "Could not copy the path for {name}.",
            { name: feedback.name },
          );
      }
    case "failed":
      return text(
        "ui.browse.copy_path_failed_for",
        "Could not copy the path for {name}.",
        { name: feedback.name },
      );
  }
}

function splitName(
  message: string,
  name: string,
): { beforeName: string; name: string; afterName: string } {
  const position = name ? message.indexOf(name) : -1;
  if (position < 0) return { beforeName: message, name: "", afterName: "" };
  return {
    beforeName: message.slice(0, position),
    name,
    afterName: message.slice(position + name.length),
  };
}
