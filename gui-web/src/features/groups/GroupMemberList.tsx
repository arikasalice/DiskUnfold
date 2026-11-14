import { useCallback, useState, useSyncExternalStore } from "react";
import type { Ref } from "react";
import {
  Action,
  BrowseGrid,
  ItemActions,
  Notice,
  Progress,
  StatusBadge,
  Text,
} from "../../components";
import type { BrowseGridFocus } from "../../components";
import type { SizeDisplay } from "../../i18n/bytes";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { GroupMemberRow } from "../../ipc";
import { memberRowKey } from "../../state/groups-members";
import type { GroupMembersReader } from "../../state/groups-members";
import { memberSizeText, memberTextValue } from "./group-format";
import { reasonLabel } from "./group-text";

function membership(
  row: GroupMemberRow,
  names: ReadonlyMap<string, string>,
  text: LocaleText,
): string {
  if (row.groups.length > 0)
    return row.groups
      .map(
        (id) =>
          names.get(id) ?? text("ui.groups.group_missing", "Unknown group"),
      )
      .join(" · ");
  if (row.undecidableReasonKeys.length > 0)
    return row.undecidableReasonKeys
      .map((key) => reasonLabel(key, text))
      .join(" ");
  return text("ui.groups.no_group", "No group");
}

/**
 * The members of one category. Rows load for what is on screen; every row
 * shows its original path, and a file in several groups is one row with a
 * "n groups" badge and the group names.
 */
export function GroupMemberList({
  reader,
  groupNames,
  display,
  onExplain,
  containerRef,
}: {
  reader: GroupMembersReader;
  groupNames: ReadonlyMap<string, string>;
  display: SizeDisplay;
  onExplain: (row: GroupMemberRow) => void;
  containerRef?: Ref<HTMLElement>;
}) {
  const text = useLocaleText();
  const snapshot = useSyncExternalStore(reader.subscribe, reader.getSnapshot);
  const [focusRequest, setFocusRequest] = useState<BrowseGridFocus>();
  const move = useCallback(
    (index: number) => {
      reader.ensure(index);
      const request: BrowseGridFocus = {
        index,
        eligible: () => true,
        finish: () =>
          setFocusRequest((current) =>
            current === request ? undefined : current,
          ),
      };
      setFocusRequest(request);
    },
    [reader],
  );
  const show = useCallback(
    (start: number, end: number) => reader.show(start, end),
    [reader],
  );
  const columns = [
    text("ui.browse.column_name", "Name"),
    text("ui.groups.col_path", "Original path"),
    text("ui.browse.column_logical_size", "Logical size"),
    text("ui.browse.column_allocated_size", "Allocated size"),
    text("ui.groups.col_groups", "Groups"),
  ];
  return (
    <section
      ref={containerRef}
      tabIndex={-1}
      className="du-groups-members"
      aria-label={text("ui.groups.members_label", "Group members")}
    >
      {snapshot.failed && (
        <Notice tone="error">
          {text("ui.groups.members_failed", "Could not load members.")}{" "}
          <Action onPress={() => reader.retry()}>
            {text("ui.groups.retry", "Try again")}
          </Action>
        </Notice>
      )}
      {snapshot.total === 0 ? (
        <Text>
          {text("ui.groups.members_empty", "No files in this category.")}
        </Text>
      ) : (
        <>
          {snapshot.loading && (
            <Progress
              label={text("ui.groups.members_loading", "Loading members…")}
            />
          )}
          <BrowseGrid<GroupMemberRow>
            label={text("ui.groups.members_grid", "Group members")}
            columns={columns}
            count={snapshot.total}
            rowCount={snapshot.total + 1}
            busy={snapshot.loading}
            focusRequest={focusRequest}
            onMove={move}
            onOpen={onExplain}
            onFocusRow={() => {}}
            onVisible={show}
            getRow={(index) => reader.row(index)}
            getKey={(row) => memberRowKey(row.item)}
            renderCells={(row) => {
              const name = memberTextValue(row.displayName, text);
              return [
                <ItemActions
                  key="name"
                  rowScope
                  label={text("ui.browse.item_actions", "Actions for {name}", {
                    name,
                  })}
                  actions={[
                    {
                      id: "explain",
                      label: text(
                        "ui.groups.explain_action",
                        "Show classification reasons",
                      ),
                      onAction: () => onExplain(row),
                    },
                  ]}
                >
                  <Action excludeFromTabOrder onPress={() => onExplain(row)}>
                    {name}
                  </Action>
                </ItemActions>,
                <Text key="path" kind="path">
                  {memberTextValue(row.displayPath, text)}
                </Text>,
                <Text key="logical">
                  {memberSizeText(row.logicalSize, display, text)}
                </Text>,
                <Text key="allocated">
                  {memberSizeText(row.allocatedSize, display, text)}
                </Text>,
                <div key="groups" className="du-groups-memberships">
                  {row.groups.length > 1 && (
                    <Action
                      variant="quiet"
                      compact
                      excludeFromTabOrder
                      aria-label={text(
                        "ui.groups.badge_action",
                        "{badge}: show classification reasons",
                        {
                          badge: text(
                            "ui.groups.group_count_badge",
                            "{count} groups",
                            { count: String(row.groups.length) },
                          ),
                        },
                      )}
                      onPress={() => onExplain(row)}
                    >
                      <StatusBadge tone="neutral">
                        {text("ui.groups.group_count_badge", "{count} groups", {
                          count: String(row.groups.length),
                        })}
                      </StatusBadge>
                    </Action>
                  )}
                  <Text>{membership(row, groupNames, text)}</Text>
                </div>,
              ];
            }}
          />
        </>
      )}
    </section>
  );
}
