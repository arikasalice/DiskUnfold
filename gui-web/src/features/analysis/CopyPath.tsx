import { useEffect, useRef, useState } from "react";
import { Action, DetailDisclosure, LiveStatus, Text } from "../../components";
import type { AnalysisClient, BrowseValue, ItemRef } from "../../ipc";
import { displaySpaceReason } from "./space-format";

type CopyPathProps = {
  client: AnalysisClient;
  item: ItemRef;
  path: BrowseValue<string>;
};

export function CopyPath(props: CopyPathProps) {
  return (
    <CopyPathForItem
      key={JSON.stringify([props.item.resultVersion, props.item.itemNo])}
      {...props}
    />
  );
}

function CopyPathForItem({ client, item, path }: CopyPathProps) {
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const active = useRef(0);
  const pending = useRef(false);
  const copyButton = useRef<HTMLButtonElement>(null);
  const dismissButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const generation = ++active.current;
    setStatus("");
    return () => {
      if (active.current === generation) active.current++;
    };
  }, [item.resultVersion, item.itemNo]);
  function copy() {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setStatus("Copying path…");
    const generation = active.current;
    void client
      .call("browse.copy_path", {
        resultVersion: item.resultVersion,
        items: [item],
      })
      .then((result) => {
        if (active.current !== generation) return;
        if (result.outcome === "copied") {
          setStatus(
            `Copied ${result.count} path${result.count === 1 ? "" : "s"}.`,
          );
        } else {
          const reasons = {
            "path.unrepresentable": "The path cannot be represented.",
            "path.multiline": "A multiline path cannot be copied.",
            "clipboard.unsupported": "The clipboard is not supported.",
            "clipboard.failed": "Could not copy to the clipboard.",
          };
          setStatus(
            Object.hasOwn(reasons, result.reasonKey)
              ? reasons[result.reasonKey]
              : "Could not copy the path.",
          );
        }
      })
      .catch(() => {
        if (active.current === generation)
          setStatus("Could not copy the path.");
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
            <DetailDisclosure title="Full path">
              <Text kind="path" focusable>
                {path.value}
              </Text>
            </DetailDisclosure>
          </div>
        ) : path.state === "absent" ? (
          "Not recorded"
        ) : (
          (displaySpaceReason(path.reasonKey) ?? "Unavailable in this scan")
        )}
        <Action ref={copyButton} aria-disabled={busy} onPress={copy}>
          Copy path
        </Action>
      </div>
      <LiveStatus>{status}</LiveStatus>
      {status && !busy && (
        <Action
          variant="quiet"
          ref={dismissButton}
          onPress={() => {
            const restoreFocus =
              document.activeElement === dismissButton.current;
            setStatus("");
            if (restoreFocus) copyButton.current?.focus();
          }}
        >
          Dismiss copy result
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
