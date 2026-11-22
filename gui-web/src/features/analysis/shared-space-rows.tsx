import { useEffect, useState } from "react";
import { Action, NameText } from "../../components";
import type { AnalysisClient, ItemRef, SpaceDetail } from "../../ipc";
import type { useLocaleText } from "../../i18n/presentation";

type LocaleText = ReturnType<typeof useLocaleText>;

/** The counted path of a shared file, read as the display path of that item. */
function CountedPath({
  client,
  counted,
  text,
}: {
  client: AnalysisClient;
  counted: ItemRef;
  text: LocaleText;
}) {
  const [path, setPath] = useState<string>();
  const { resultVersion, itemNo } = counted;
  useEffect(() => {
    let active = true;
    void client
      .call("browse.detail", { resultVersion, item: { resultVersion, itemNo } })
      .then((detail) => {
        if (active && detail.displayPath.state === "present")
          setPath(detail.displayPath.value);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [client, resultVersion, itemNo]);
  return path === undefined ? (
    text("ui.browse.counted_at", "Counted at")
  ) : (
    <NameText
      path
      name={text("ui.browse.counted_at_path", "Counted at {path}", { path })}
    />
  );
}

/**
 * The "Counted at" row of a file whose allocated space is counted once for several
 * paths (F03 #359). A path that is not the counted one shows where it is counted and
 * can open the folder of that path; the counted path says "This path" and has no
 * button.
 */
export function sharedSpaceRows(
  detail: SpaceDetail,
  client: AnalysisClient,
  navigateItem: ((item: ItemRef) => void) | undefined,
  text: LocaleText,
) {
  const group =
    detail.linkGroup.state === "present" ? detail.linkGroup.value : undefined;
  if (group?.sharesSpace !== true) return [];
  const { countedItem, countedFolder } = group;
  return [
    {
      label: text("ui.analysis.space_counted_at", "Counted at"),
      value: countedItem ? (
        <>
          <CountedPath client={client} counted={countedItem} text={text} />
          {countedFolder && navigateItem ? (
            <Action variant="quiet" onPress={() => navigateItem(countedFolder)}>
              {text("ui.browse.counted_at_open", "Open folder")}
            </Action>
          ) : null}
        </>
      ) : (
        text("ui.analysis.space_counted_this_path", "This path")
      ),
      notes: [
        text(
          "ui.analysis.space_counted_once_note",
          "The same file is at more than one path, so it is counted at one of them.",
        ),
      ],
    },
  ];
}
