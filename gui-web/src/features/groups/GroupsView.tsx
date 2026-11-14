import "./groups.css";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { KeyboardEvent, RefObject } from "react";
import {
  Action,
  Readout,
  ReviewSurface,
  isImeProcessingKey,
} from "../../components";
import type { SizeDisplay } from "../../i18n/bytes";
import { useLocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import type { AnalysisClient, GroupSummary, ItemRef } from "../../ipc";
import { GroupRuleEditorArea } from "./GroupRuleEditor";
import { GroupRulesLine } from "./GroupRulesLine";
import {
  GroupAppliedResult,
  GroupResults,
  selectedGroup,
} from "./GroupsResults";
import type { ResultSelection } from "./GroupsResults";
import { GroupsStatus } from "./GroupsStatus";
import { useGroupsSession } from "./use-groups-session";
import type { GroupsSession } from "./use-groups-session";

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

/**
 * Focus goes to the screen title when the screen opens, to the editor when it
 * opens and back to the title when it closes. The title and the editor take
 * the focus without a frame around the screen.
 */
function useEditorFocus(
  editing: boolean,
  title: RefObject<HTMLElement | null>,
  editorArea: RefObject<HTMLElement | null>,
) {
  const wasEditing = useRef(false);
  useEffect(() => {
    title.current?.focus();
  }, [title]);
  useEffect(() => {
    if (editing && !wasEditing.current) editorArea.current?.focus();
    if (!editing && wasEditing.current) title.current?.focus();
    wasEditing.current = editing;
  }, [editing, title, editorArea]);
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
  const totals = useSyncExternalStore(
    session.totals.subscribe,
    session.totals.getSnapshot,
  );
  const [selection, setSelection] = useState<ResultSelection>({
    tab: "groups",
    groupId: undefined,
  });
  const title = useRef<HTMLHeadingElement>(null);
  const editorArea = useRef<HTMLElement>(null);
  const membersRef = useRef<HTMLElement>(null);
  const editing = editor.phase !== "viewing";
  const summary = snapshot.summary;
  const classificationRef = summary?.classificationRef;
  const resultVersion = summary?.resultVersion;
  useEditorFocus(editing, title, editorArea);
  // The table sizes and the editor's applied preview both read these totals.
  useEffect(() => {
    if (classificationRef !== undefined && resultVersion !== undefined)
      session.totals.load({ classificationRef, resultVersion });
  }, [session, classificationRef, resultVersion]);
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (
      event.key !== "Escape" ||
      isImeProcessingKey(event.nativeEvent) ||
      !editing ||
      editor.confirmingDiscard
    )
      return;
    event.preventDefault();
    event.stopPropagation();
    session.editor.requestClose();
  }
  const noRules = snapshot.ruleset?.rules.length === 0;
  return (
    <div
      className="du-groups"
      onKeyDown={onKeyDown}
      data-editing={editing || undefined}
    >
      <ReviewSurface
        title={text("ui.groups.title", "Group by my rules")}
        titleRef={title}
        description={text(
          "ui.groups.subtitle",
          "Virtual groups do not change where files are.",
        )}
        context={<ResultContext summary={summary} scope={snapshot.scope} />}
        tools={
          <Action isDisabled={editing} onPress={onClose}>
            {text("ui.groups.back_to_folders", "Back to folders")}
          </Action>
        }
        footer={
          summary && !editing ? (
            <Action
              variant="primary"
              onPress={() => membersRef.current?.focus()}
            >
              {text("ui.groups.members_action", "Member list")}
            </Action>
          ) : undefined
        }
      >
        {!editing && (
          <GroupRulesLine
            session={session}
            emphasizeEdit={noRules && summary === undefined}
            groupId={
              summary && selectedGroup(summary, selection.groupId)?.groupId
            }
          />
        )}
        <GroupsStatus
          snapshot={snapshot}
          noRules={noRules}
          onClassify={() => void session.controller.classify()}
          onCancel={() => void session.controller.cancel()}
        />
        <div className="du-groups-layout" data-editing={editing || undefined}>
          {editing ? (
            <GroupAppliedResult
              summary={summary}
              totals={totals}
              display={display}
            />
          ) : (
            summary && (
              <div className="du-groups-results">
                <GroupResults
                  session={session}
                  summary={summary}
                  display={display}
                  totals={totals}
                  selection={selection}
                  onSelect={setSelection}
                  membersRef={membersRef}
                />
              </div>
            )
          )}
          <GroupRuleEditorArea
            editor={session.editor}
            preview={session.preview}
            system={display.system}
            display={display}
            summary={summary}
            totals={totals}
            containerRef={editorArea}
          />
        </div>
      </ReviewSurface>
    </div>
  );
}

/**
 * The virtual-group screen: classifies the browsed result with the user's rules
 * and shows groups, members, reasons and totals. It never changes files. The
 * browse screen opens it only for observed (real) results; a sample result
 * keeps the entry disabled until the sample view of this screen ships.
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
