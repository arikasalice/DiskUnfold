import { useEffect, useRef, useState } from "react";
import { Action, DetailDisclosure, LiveStatus, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type {
  AnalysisClient,
  BrowseCopyPathOutcome,
  BrowseValue,
  ItemRef,
} from "../../ipc";

type CopyPathProps = {
  client: AnalysisClient;
  item: ItemRef;
  path: BrowseValue<string>;
};

type CopyPathRefusal = Extract<
  BrowseCopyPathOutcome,
  { outcome: "refused" }
>["reasonKey"];
type CopyStatus =
  | { kind: "copying" }
  | { kind: "copied"; count: number }
  | { kind: "refused"; reasonKey: CopyPathRefusal }
  | { kind: "failed" };

function copyStatusText(
  status: CopyStatus | undefined,
  text: ReturnType<typeof useLocaleText>,
): string {
  if (!status) return "";
  switch (status.kind) {
    case "copying":
      return text("ui.analysis.copying_path", "Copying path…");
    case "copied":
      return text(
        "ui.analysis.copied_paths",
        status.count === 1 ? "Copied 1 path." : "Copied {count} paths.",
        { count: BigInt(status.count) },
      );
    case "refused":
      switch (status.reasonKey) {
        case "path.unrepresentable":
          return text(
            "ui.analysis.path_unrepresentable",
            "The path cannot be represented.",
          );
        case "path.multiline":
          return text(
            "ui.analysis.path_multiline",
            "A multiline path cannot be copied.",
          );
        case "clipboard.unsupported":
          return text(
            "ui.analysis.clipboard_unsupported",
            "The clipboard is not supported.",
          );
        case "clipboard.failed":
          return text(
            "ui.analysis.clipboard_failed",
            "Could not copy to the clipboard.",
          );
      }
    case "failed":
      return text("ui.analysis.copy_path_failed", "Could not copy the path.");
  }
}

function pathReasonText(
  reasonKey: string,
  text: ReturnType<typeof useLocaleText>,
) {
  switch (reasonKey) {
    case "space.aux_not_included":
      return text(
        "ui.analysis.space_aux_not_included",
        "Auxiliary data was not collected",
      );
    case "space.dedup_capacity":
      return text(
        "ui.analysis.space_dedup_capacity",
        "The file identity limit was reached",
      );
    case "space.sharing_not_collected":
      return text(
        "ui.analysis.space_sharing_not_collected",
        "Sharing evidence was not collected",
      );
    case "space.sharing_unknown":
      return text(
        "ui.analysis.space_sharing_unknown",
        "Sharing could not be verified",
      );
    case "space.identity_unsupported":
      return text(
        "ui.analysis.space_identity_unsupported",
        "File identity could not be verified",
      );
    case "space.out_of_range_links":
      return text(
        "ui.analysis.space_out_of_range_links",
        "Links outside the scan were reported",
      );
    case "space.shared_reported":
      return text(
        "ui.analysis.space_shared_reported",
        "The file system reported shared space",
      );
    default:
      return text(
        "ui.inspection.unavailable_in_scan",
        "Unavailable in this scan",
      );
  }
}

export function CopyPath(props: CopyPathProps) {
  return (
    <CopyPathForItem
      key={JSON.stringify([props.item.resultVersion, props.item.itemNo])}
      {...props}
    />
  );
}

function CopyPathForItem({ client, item, path }: CopyPathProps) {
  const text = useLocaleText();
  const [status, setStatus] = useState<CopyStatus>();
  const statusText = copyStatusText(status, text);
  const [busy, setBusy] = useState(false);
  const active = useRef(0);
  const pending = useRef(false);
  const copyButton = useRef<HTMLButtonElement>(null);
  const dismissButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const generation = ++active.current;
    setStatus(undefined);
    return () => {
      if (active.current === generation) active.current++;
    };
  }, [item.resultVersion, item.itemNo]);
  function copy() {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setStatus({ kind: "copying" });
    const generation = active.current;
    void client
      .call("browse.copy_path", {
        resultVersion: item.resultVersion,
        items: [item],
      })
      .then((result) => {
        if (active.current !== generation) return;
        if (result.outcome === "copied") {
          setStatus({ kind: "copied", count: result.count });
        } else {
          setStatus({ kind: "refused", reasonKey: result.reasonKey });
        }
      })
      .catch(() => {
        if (active.current === generation) setStatus({ kind: "failed" });
      })
      .finally(() => {
        pending.current = false;
        if (active.current === generation) setBusy(false);
      });
  }
  return (
    <>
      <div className="du-copy-path">
        {path.state === "present" ? (
          <div className="du-inspection-path">
            <Text kind="path">{abbreviate(path.value)}</Text>
            <DetailDisclosure
              title={text("ui.analysis.full_path", "Full path")}
            >
              <Text kind="path" focusable>
                {path.value}
              </Text>
            </DetailDisclosure>
          </div>
        ) : path.state === "absent" ? (
          text("ui.analysis.not_recorded", "Not recorded")
        ) : (
          pathReasonText(path.reasonKey, text)
        )}
        <Action ref={copyButton} aria-disabled={busy} onPress={copy}>
          {text("ui.browse.copy_path", "Copy path")}
        </Action>
      </div>
      <LiveStatus>{statusText}</LiveStatus>
      {status && !busy && (
        <Action
          variant="quiet"
          ref={dismissButton}
          onPress={() => {
            const restoreFocus =
              document.activeElement === dismissButton.current;
            setStatus(undefined);
            if (restoreFocus) copyButton.current?.focus();
          }}
        >
          {text("ui.analysis.dismiss_copy_result", "Dismiss copy result")}
        </Action>
      )}
    </>
  );
}

function abbreviate(path: string): string {
  const characters = Array.from(path);
  return characters.length <= 40
    ? path
    : `${characters.slice(0, 19).join("")}…${characters.slice(-19).join("")}`;
}
