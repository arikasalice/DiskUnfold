import type { ComponentProps } from "react";
import { Choice, ChoiceGroup } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { GroupSummary } from "../../ipc";

type IconName = ComponentProps<typeof Choice>["icon"];

/** The member categories a classification offers as tabs. */
export type CategoryTab = "groups" | "overlap" | "unclassified" | "undecidable";

const TABS: readonly CategoryTab[] = [
  "groups",
  "overlap",
  "unclassified",
  "undecidable",
];

/** Narrows an arbitrary string to a tab. */
export function asCategoryTab(value: string): CategoryTab | undefined {
  return TABS.find((tab) => tab === value);
}

/** Category tabs with their counts, built from the common choice group. */
export function GroupCategoryTabs({
  summary,
  value,
  onChange,
}: {
  summary: GroupSummary;
  value: CategoryTab;
  onChange: (tab: CategoryTab) => void;
}) {
  const text = useLocaleText();
  const items = (count: number) =>
    text("ui.groups.items_count", "{count} items", { count: String(count) });
  const details: Record<
    CategoryTab,
    { label: string; description: string; count: number; icon: IconName }
  > = {
    groups: {
      label: text("ui.groups.tab_groups", "Groups"),
      description: text("ui.groups.tab_groups_desc", "Files a rule matched"),
      count: summary.matchedCount,
      icon: "layers",
    },
    overlap: {
      label: text("ui.groups.tab_overlap", "Overlaps"),
      description: text(
        "ui.groups.tab_overlap_desc",
        "Files in more than one group",
      ),
      count: summary.overlapCount,
      icon: "copy",
    },
    unclassified: {
      label: text("ui.groups.tab_unclassified", "Unclassified"),
      description: text(
        "ui.groups.tab_unclassified_desc",
        "Files no rule matched",
      ),
      count: summary.unclassifiedCount,
      icon: "folder",
    },
    undecidable: {
      label: text("ui.groups.tab_undecidable", "Cannot decide"),
      description: text(
        "ui.groups.tab_undecidable_desc",
        "Files a rule could not decide",
      ),
      count: summary.undecidableCount,
      icon: "alert",
    },
  };
  return (
    <ChoiceGroup
      id="du-groups-categories"
      label={text("ui.groups.tabs_label", "Categories")}
      value={value}
      isDisabled={false}
      onChange={(next) => {
        const tab = asCategoryTab(next);
        if (tab) onChange(tab);
      }}
    >
      {TABS.map((tab) => (
        <Choice
          key={tab}
          value={tab}
          label={details[tab].label}
          description={details[tab].description}
          icon={details[tab].icon}
          status={{ text: items(details[tab].count), tone: "neutral" }}
        />
      ))}
    </ChoiceGroup>
  );
}
