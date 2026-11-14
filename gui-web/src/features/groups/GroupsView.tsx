import "./groups.css";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { KeyboardEvent, RefObject } from "react";
import {
  Action,
  Heading,
  Notice,
  Progress,
  Readout,
  ReviewSurface,
  StatusBadge,
} from "../../components";
import type { SizeDisplay } from "../../i18n/bytes";
import { useLocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import type {
  AnalysisClient,
  GroupCategory,
  GroupMemberRow,
  GroupSummary,
  ItemRef,
} from "../../ipc";
import { memberRowKey } from "../../state/groups-members";
import type { MembersView } from "../../state/groups-members";
import { GroupCategoryTabs } from "./GroupCategoryTabs";
import type { CategoryTab } from "./GroupCategoryTabs";
import { GroupExplainPanel } from "./GroupExplainPanel";
import { GroupLimitations } from "./GroupLimitations";
import { GroupMemberList } from "./GroupMemberList";
import { GroupRuleEditorArea } from "./GroupRuleEditor";
import { GroupSummaryTable } from "./GroupSummaryTable";
import { GroupTotalsLine } from "./GroupTotalsLine";
import { GroupsStatus } from "./GroupsStatus";
import { useGroupsSession } from "./use-groups-session";
import type { GroupsSession } from "./use-groups-session";

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
  const group =
    summary.groups.find((row) => row.groupId === groupId) ?? summary.groups[0];
  return group
    ? make({ kind: "group", groupId: group.groupId }, group.memberCount)
    : undefined;
}

function RulesLine({ session }: { session: GroupsSession }) {
  const text = useLocaleText();
  const snapshot = useSyncExternalStore(
    session.controller.subscribe,
    session.controller.getSnapshot,
  );
  const { ruleset, rulesStatus } = snapshot;
  const names = ruleset?.rules.map((rule) => rule.displayName).join(", ");
  return (
    <section
      className="du-groups-rules-line"
      aria-label={text("ui.groups.rules_label", "Rules")}
    >
      {ruleset && (
        <Readout
          label={text("ui.groups.rules_label", "Rules")}
          rows={[
            {
              label: text("ui.groups.rules_label", "Rules"),
              value:
                names ||
                text("ui.groups.rules_none", "No rules are saved yet."),
              wideValue: true,
            },
            {
              label: text("ui.groups.rules_version_label", "Rules version"),
              value: String(ruleset.rulesetVersion),
            },
          ]}
        />
      )}
      {rulesStatus === "loading" && (
        <Progress
          label={text("ui.groups.rules_loading", "Loading the rules…")}
        />
      )}
      {rulesStatus === "error" && (
        <Notice tone="error">
          {text("ui.groups.rules_failed", "Could not read the rules.")}{" "}
          <Action onPress={() => void session.controller.refreshRules()}>
            {text("ui.groups.retry", "Try again")}
          </Action>
        </Notice>
      )}
      <Action
        isDisabled={!ruleset}
        onPress={() => ruleset && session.editor.open(ruleset)}
      >
        {text("ui.groups.edit_rules", "Edit rules")}
      </Action>
    </section>
  );
}

function ResultContext({
  summary,
  scope,
}: {
  summary: GroupSummary | undefined;
  scope: ItemRef | undefined;
}) {
  const text = useLocaleText();
  const timestamp = useTimestampText();
  const observed = summary?.observedAt;
  const rows = [
    {
      label: text("ui.groups.scope_label", "Scope"),
      value: scope
        ? text("ui.groups.scope_folder", "The folder you were browsing")
        : text("ui.groups.scope_result", "The whole result"),
    },
    ...(summary
      ? [
          {
            label: text("ui.browse.observed", "Observed"),
            value:
              observed === undefined
                ? text("ui.browse.unavailable", "Unavailable")
                : timestamp(Date.parse(observed), () => observed),
          },
          {
            label: text("ui.browse.completeness", "Completeness"),
            value:
              summary.completeness === "complete"
                ? text("ui.browse.complete", "Complete")
                : text("ui.browse.incomplete", "Incomplete"),
          },
        ]
      : []),
  ];
  return (
    <Readout
      label={text("ui.groups.context_label", "Classified result")}
      rows={rows}
    />
  );
}

function MembersHeading({
  summary,
  tab,
  groupId,
}: {
  summary: GroupSummary;
  tab: CategoryTab;
  groupId: string | undefined;
}) {
  const text = useLocaleText();
  const group =
    summary.groups.find((row) => row.groupId === groupId) ?? summary.groups[0];
  const title =
    tab === "groups"
      ? (group?.displayName ?? text("ui.groups.tab_groups", "Groups"))
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
          {text("ui.groups.virtual_badge", "Virtual group")}
        </StatusBadge>
      )}
    </div>
  );
}

function Results({
  session,
  summary,
  display,
  membersRef,
}: {
  session: GroupsSession;
  summary: GroupSummary;
  display: SizeDisplay;
  membersRef: RefObject<HTMLElement | null>;
}) {
  const text = useLocaleText();
  const [tab, setTab] = useState<CategoryTab>("groups");
  const [groupId, setGroupId] = useState<string>();
  const [explained, setExplained] = useState<ItemRef>();
  const totals = useSyncExternalStore(
    session.totals.subscribe,
    session.totals.getSnapshot,
  );
  const explain = useSyncExternalStore(
    session.explain.subscribe,
    session.explain.getSnapshot,
  );
  const { classificationRef, resultVersion } = summary;
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
    session.totals.load({ classificationRef, resultVersion });
  }, [session, classificationRef, resultVersion]);
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
      <GroupCategoryTabs summary={summary} value={tab} onChange={setTab} />
      {tab === "groups" && (
        <GroupSummaryTable
          summary={summary}
          totals={totals}
          display={display}
          selected={
            view?.category.kind === "group" ? view.category.groupId : undefined
          }
          selectable
          onSelect={setGroupId}
        />
      )}
      <MembersHeading summary={summary} tab={tab} groupId={groupId} />
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

function Screen({
  session,
  display,
  onClose,
}: {
  session: GroupsSession;
  display: SizeDisplay;
  onClose: () => void;
}) {
  const text = useLocaleText();
  const snapshot = useSyncExternalStore(
    session.controller.subscribe,
    session.controller.getSnapshot,
  );
  const editor = useSyncExternalStore(
    session.editor.subscribe,
    session.editor.getSnapshot,
  );
  const root = useRef<HTMLDivElement>(null);
  const editorArea = useRef<HTMLElement>(null);
  const membersRef = useRef<HTMLElement>(null);
  const editing = editor.phase !== "viewing";
  const wasEditing = useRef(false);
  useEffect(() => {
    root.current?.focus();
  }, []);
  useEffect(() => {
    if (editing && !wasEditing.current) editorArea.current?.focus();
    // Closing the editor returns focus to the screen's first stop.
    if (!editing && wasEditing.current) root.current?.focus();
    wasEditing.current = editing;
  }, [editing]);
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (
      event.key !== "Escape" ||
      event.nativeEvent.isComposing ||
      !editing ||
      editor.confirmingDiscard
    )
      return;
    event.preventDefault();
    event.stopPropagation();
    session.editor.requestClose();
  }
  const summary = snapshot.summary;
  return (
    <div
      ref={root}
      tabIndex={-1}
      className="du-groups"
      onKeyDown={onKeyDown}
      data-editing={editing || undefined}
    >
      <ReviewSurface
        title={text("ui.groups.title", "Group by my rules")}
        description={text(
          "ui.groups.subtitle",
          "Virtual groups do not change where files are.",
        )}
        context={<ResultContext summary={summary} scope={snapshot.scope} />}
        footer={
          <>
            <Action icon="back" onPress={onClose}>
              {text("ui.groups.back_to_folders", "Back to folders")}
            </Action>
            {summary && !editing && (
              <Action
                variant="primary"
                onPress={() => membersRef.current?.focus()}
              >
                {text("ui.groups.members_action", "Member list")}
              </Action>
            )}
          </>
        }
      >
        <RulesLine session={session} />
        <GroupsStatus
          snapshot={snapshot}
          onClassify={() => void session.controller.classify()}
          onCancel={() => void session.controller.cancel()}
        />
        <GroupRuleEditorArea
          editor={session.editor}
          system={display.system}
          containerRef={editorArea}
        />
        {!editing && summary && (
          <Results
            key={summary.classificationRef}
            session={session}
            summary={summary}
            display={display}
            membersRef={membersRef}
          />
        )}
      </ReviewSurface>
    </div>
  );
}

/**
 * The virtual-group screen: classifies the browsed result with the user's rules
 * and shows groups, members, reasons and totals. It never changes files.
 */
export function GroupsView({
  client,
  resultVersion,
  scope,
  display,
  onClose,
}: {
  client: AnalysisClient;
  resultVersion: string;
  scope: ItemRef | undefined;
  display: SizeDisplay;
  onClose: () => void;
}) {
  const session = useGroupsSession(
    client,
    resultVersion,
    scope,
    display.system,
  );
  if (!session) return null;
  return <Screen session={session} display={display} onClose={onClose} />;
}
