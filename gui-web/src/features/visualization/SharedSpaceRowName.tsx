import { useRef, useState } from "react";
import type { AnalysisClient, VizModel, VizRow } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import { RowName } from "../../viz";
import type { ModelRowsPage, VizText } from "../../viz";

/** Looks up the counted path only when its shared row is focused or hovered. */
export function SharedCountedRowName({
  row,
  name,
  folder,
  client,
  enabled,
}: {
  row: VizRow;
  name: string;
  folder: boolean;
  client: AnalysisClient | undefined;
  enabled: boolean;
}) {
  const localeText = useLocaleText();
  const counted = enabled ? row.sharedCountedItem : undefined;
  const key = counted
    ? JSON.stringify([counted.resultVersion, counted.itemNo])
    : undefined;
  const current = useRef({ client, key });
  current.current = { client, key };
  const requested = useRef<
    | {
        client: AnalysisClient;
        key: string;
        status: "pending" | "loaded" | "failed";
      }
    | undefined
  >(undefined);
  const [loaded, setLoaded] = useState<{
    client: AnalysisClient;
    key: string;
    path: string;
  }>();
  const path =
    client && key && loaded?.client === client && loaded.key === key
      ? loaded.path
      : undefined;
  const loadPath = () => {
    if (!client || !counted || !key) return;
    const previous = requested.current;
    if (
      previous?.client === client &&
      previous.key === key &&
      previous.status !== "failed"
    )
      return;
    const owner = { client, key, status: "pending" as const };
    requested.current = owner;
    void client
      .call("browse.detail", {
        resultVersion: counted.resultVersion,
        item: counted,
      })
      .then((detail) => {
        if (
          requested.current !== owner ||
          current.current.client !== client ||
          current.current.key !== key
        )
          return;
        if (detail.displayPath.state === "present") {
          requested.current = { ...owner, status: "loaded" };
          setLoaded({ client, key, path: detail.displayPath.value });
        } else requested.current = { ...owner, status: "failed" };
      })
      .catch(() => {
        if (requested.current === owner)
          requested.current = { ...owner, status: "failed" };
      });
  };
  return (
    <RowName
      name={name}
      item={row.itemRef !== null}
      folder={folder}
      {...(path === undefined
        ? {}
        : {
            title: localeText(
              "ui.browse.counted_at_path",
              "Counted at {path}",
              {
                path,
              },
            ),
          })}
      {...(client && counted
        ? { tabIndex: 0, onPointerEnter: loadPath, onFocus: loadPath }
        : {})}
    />
  );
}

/** Whether both the page and requested model use the allocated basis. */
export function sharedFieldsApply(
  response: ModelRowsPage["response"],
  model: VizModel | undefined,
) {
  return (
    response.sizeBasis === "allocated" &&
    model?.representation.sizeBasis === "allocated"
  );
}

/** The row's statement that this item shares allocated space. */
export function sharedReason(row: VizRow, text: VizText, sharedShown: boolean) {
  if (!sharedShown || row.sharesSpace !== true) return null;
  return text.message(
    row.sharedCountedItem === undefined
      ? "sharesSpaceCountedHere"
      : "sharesSpace",
  );
}
