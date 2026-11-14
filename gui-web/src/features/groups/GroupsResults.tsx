import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import type { RefObject } from "react";
import { Heading, Notice, StatusBadge, Text } from "../../components";
import type { SizeDisplay } from "../../i18n/bytes";
import { useLocaleText } from "../../i18n/presentation";
import type {
  GroupCategory,
  GroupMemberRow,
  GroupSummary,
  GroupTotals,
  ItemRef,
} from "../../ipc";
import type { ReadState, TotalsKey } from "../../state/groups-details";
import { memberRowKey } from "../../state/groups-members";
import type { MembersView } from "../../state/groups-members";
import { GroupCategoryTabs } from "./GroupCategoryTabs";
import type { CategoryTab } from "./GroupCategoryTabs";
import { GroupExplainPanel } from "./GroupExplainPanel";
import { GroupLimitations } from "./GroupLimitations";
import { GroupMemberList } from "./GroupMemberList";
import { GroupSummaryTable } from "./GroupSummaryTable";
import { GroupTotalsLine } from "./GroupTotalsLine";
import type { GroupsSession } from "./use-groups-session";

/** The tab and group the user chose; the screen keeps it while the editor is open. */
export type ResultSelection = { tab: CategoryTab; groupId: string | undefined };

/** The group a selection stands for: the chosen one, else the first. */
export function selectedGroup(
  summary: GroupSummary,
  groupId: string | undefined,
) {
  return (
    summary.groups.find((row) => row.groupId === groupId) ?? summary.groups[0]
  );
}

/** The member list a tab and group selection stand for. */
function membersViewOf(
  summary: GroupSummary,
  tab: CategoryTab,
  groupId: string | undefined,
): MembersView | undefined {
  const base = {
    classificationRef: summary.classificationRef,
    resultVersion: summary.resultVersion,
  };
  const make = (category: GroupCategory, count: number): MembersView => ({
    ...base,
    category,
    count,
  });
  if (tab === "overlap") return make({ kind: "overlap" }, summary.overlapCount);
  if (tab === "unclassified")
    return make({ kind: "unclassified" }, summary.unclassifiedCount);
  if (tab === "undecidable")
    return make({ kind: "undecidable" }, summary.undecidableCount);
  const group = selectedGroup(summary, groupId);
  return group
    ? make({ kind: "group", groupId: group.groupId }, group.memberCount)
    : undefined;
}

function MembersHeading({
  summary,
  selection,
}: {
  summary: GroupSummary;
  selection: ResultSelection;
}) {
  const text = useLocaleText();
  const { tab } = selection;
  const group = selectedGroup(summary, selection.groupId);
  const title =
    tab === "groups"
      ? group
        ? group.displayName
        : text("ui.groups.tab_groups", "Groups")
      : tab === "overlap"
        ? text("ui.groups.tab_overlap", "Overlaps")
        : tab === "unclassified"
          ? text("ui.groups.tab_unclassified", "Unclassified")
          : text("ui.groups.tab_undecidable", "Cannot decide");
  return (
    <div className="du-groups-name">
      <Heading level={3}>
        {text("ui.groups.members_of", "Members: {name}", { name: title })}
      </Heading>
      {tab === "groups" && group && (
        <StatusBadge tone="info">
          {text("ui.groups.virtual_badge", "Virtual")}
        </StatusBadge>
      )}
    </div>
  );
}

/**
 * The classified result: limits, tabs, the summary table, the member list, the
 * reasons of one member and the combined total.
 */
export function GroupResults({
  session,
  summary,
  display,
  totals,
  selection,
  onSelect,
  membersRef,
}: {
  session: GroupsSession;
  summary: GroupSummary;
  display: SizeDisplay;
  totals: ReadState<TotalsKey, GroupTotals>;
  selection: ResultSelection;
  onSelect: (selection: ResultSelection) => void;
  membersRef: RefObject<HTMLElement | null>;
}) {
  const text = useLocaleText();
  const [explained, setExplained] = useState<ItemRef>();
  const explain = useSyncExternalStore(
    session.explain.subscribe,
    session.explain.getSnapshot,
  );
  const { classificationRef, resultVersion } = summary;
  const { tab, groupId } = selection;
  const view = useMemo(
    () => membersViewOf(summary, tab, groupId),
    // A new classification replaces the summary; the key fields identify it.
    [classificationRef, resultVersion, summary.groups, tab, groupId],
  );
  const viewKey = view
    ? `${view.classificationRef}|${view.category.kind}|${view.category.kind === "group" ? view.category.groupId : ""}|${view.count}`
    : "";
  useEffect(() => {
    session.members.setView(view);
  }, [session, viewKey]);
  useEffect(() => {
    setExplained(undefined);
    session.explain.clear();
  }, [session, classificationRef]);
  const names = useMemo(
    () => new Map(summary.groups.map((row) => [row.groupId, row.displayName])),
    [summary.groups],
  );
  const explainRow = (row: GroupMemberRow) => {
    setExplained(row.item);
    session.explain.load({ classificationRef, item: row.item });
  };
  return (
    <>
      <GroupLimitations limitations={summary.limitations} />
      {summary.groups.some((row) => row.ruleUnevaluable) && (
        <Notice>
          {text(
            "ui.groups.unevaluable_notice",
            "A rule uses a condition that is not registered, so some files cannot be decided. They are listed under Cannot decide.",
          )}
        </Notice>
      )}
      {summary.matchedCount === 0 && (
        <Notice>
          {text("ui.groups.empty", "No items match these rules.")}
        </Notice>
      )}
      <GroupCategoryTabs
        summary={summary}
        value={tab}
        onChange={(next) => onSelect({ tab: next, groupId })}
      />
      {tab === "groups" && (
        <GroupSummaryTable
          summary={summary}
          totals={totals}
          display={display}
          selected={
            view?.category.kind === "group" ? view.category.groupId : undefined
          }
          selectable
          onSelect={(next) => onSelect({ tab, groupId: next })}
        />
      )}
      <MembersHeading summary={summary} selection={selection} />
      <GroupMemberList
        reader={session.members}
        groupNames={names}
        display={display}
        onExplain={explainRow}
        containerRef={membersRef}
      />
      {explained && (
        <GroupExplainPanel
          key={memberRowKey(explained)}
          state={explain}
          groupNames={names}
          onRetry={() => session.explain.retry()}
          onClose={() => {
            setExplained(undefined);
            session.explain.clear();
            membersRef.current?.focus();
          }}
        />
      )}
      <GroupTotalsLine
        state={totals}
        display={display}
        onRetry={() => session.totals.retry()}
      />
    </>
  );
}

/**
 * Beside the rule editor: the result that is applied now, unchanged while the
 * rules are being edited.
 */
export function GroupAppliedResult({
  summary,
  totals,
  display,
}: {
  summary: GroupSummary | undefined;
  totals: ReadState<TotalsKey, GroupTotals>;
  display: SizeDisplay;
}) {
  const text = useLocaleText();
  return (
    <div className="du-groups-applied">
      {summary && (
        <GroupSummaryTable
          applied
          summary={summary}
          totals={totals}
          display={display}
          selected={undefined}
          selectable={false}
          onSelect={() => {}}
        />
      )}
      <Text kind="muted">
        {summary
          ? text(
              "ui.groups.editing_note",
              "While you edit, the result of the rules applied now stays as it is.",
            )
          : text(
              "ui.groups.editing_no_result",
              "Nothing has been classified yet.",
            )}
      </Text>
    </div>
  );
}
