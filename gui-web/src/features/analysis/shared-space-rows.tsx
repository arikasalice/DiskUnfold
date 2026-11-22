import { Action } from "../../components";
import type { ItemRef, SpaceDetail } from "../../ipc";
import type { useLocaleText } from "../../i18n/presentation";

type LocaleText = ReturnType<typeof useLocaleText>;

/**
 * The extra readout rows of a file whose allocated space is counted once for several
 * paths (F03 #359). The path that is not the counted one points to it and can open the
 * folder of the counted path only; the counted path has no reference to follow.
 */
export function sharedSpaceRows(
  detail: SpaceDetail,
  navigateItem: ((item: ItemRef) => void) | undefined,
  text: LocaleText,
) {
  const group =
    detail.linkGroup.state === "present" ? detail.linkGroup.value : undefined;
  if (group?.sharesSpace !== true) return [];
  const counted = group.countedItem;
  return [
    {
      label: text("ui.analysis.space_shared_allocation", "Shared allocation"),
      value: counted
        ? text(
            "ui.analysis.space_shares_with_other_path",
            "Shares space with another path",
          )
        : text(
            "ui.analysis.space_counted_here",
            "Counted here, shared with another path",
          ),
    },
    ...(counted && navigateItem
      ? [
          {
            label: text(
              "ui.analysis.space_counted_elsewhere",
              "Counted at another path",
            ),
            value: (
              <Action variant="quiet" onPress={() => navigateItem(counted)}>
                {text("ui.browse.counted_at_open", "Open folder")}
              </Action>
            ),
          },
        ]
      : []),
  ];
}
