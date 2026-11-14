import type { ReactNode } from "react";
import { Tabs } from "../../components";
import type { TabsItem } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { GroupSummary } from "../../ipc";

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

/**
 * Category tabs with their counts, on the common tab part. The part draws the
 * selected tab bold with an underline and keeps the selection off colour alone.
 * `children` is the tab panel: the summary, members and totals of the chosen
 * category, labelled by the selected tab.
 */
export function GroupCategoryTabs({
  summary,
  value,
  onChange,
  children,
}: {
  summary: GroupSummary;
  value: CategoryTab;
  onChange: (tab: CategoryTab) => void;
  children?: ReactNode;
}) {
  const text = useLocaleText();
  const count = (tab: CategoryTab): number =>
    tab === "groups"
      ? summary.groups.length
      : tab === "overlap"
        ? summary.overlapCount
        : tab === "unclassified"
          ? summary.unclassifiedCount
          : summary.undecidableCount;
  const details: Record<CategoryTab, { label: string; description: string }> = {
    groups: {
      label: text("ui.groups.tab_groups", "Groups"),
      description: text(
        "ui.groups.tab_groups_desc",
        "The groups of your rules",
      ),
    },
    overlap: {
      label: text("ui.groups.tab_overlap", "Overlaps"),
      description: text(
        "ui.groups.tab_overlap_desc",
        "Files in more than one group",
      ),
    },
    unclassified: {
      label: text("ui.groups.tab_unclassified", "Unclassified"),
      description: text(
        "ui.groups.tab_unclassified_desc",
        "Files no rule matched",
      ),
    },
    undecidable: {
      label: text("ui.groups.tab_undecidable", "Cannot decide"),
      description: text(
        "ui.groups.tab_undecidable_desc",
        "Files a rule could not decide",
      ),
    },
  };
  const items: readonly TabsItem<CategoryTab>[] = TABS.map((tab) => ({
    key: tab,
    label: details[tab].label,
    description: details[tab].description,
    count: text("ui.groups.tab_count", "{count}", {
      count: String(count(tab)),
    }),
  }));
  return (
    <Tabs
      label={text("ui.groups.tabs_label", "Categories")}
      items={items}
      value={value}
      onChange={onChange}
    >
      {children}
    </Tabs>
  );
}
