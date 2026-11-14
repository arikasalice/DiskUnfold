import { Action, DataTable, StatusBadge, Text } from "../../components";
import type { SizeDisplay } from "../../i18n/bytes";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { GroupSummary, GroupSummaryRow, GroupTotals } from "../../ipc";
import type { ReadState, TotalsKey } from "../../state/groups-details";
import { isLowerBound, rowCountTexts, totalSizeText } from "./group-format";
import type { ReactNode } from "react";

type TableRow =
  | { kind: "group"; key: string; row: GroupSummaryRow }
  | { kind: "unclassified"; key: string };

type Totals = ReadState<TotalsKey, GroupTotals>;

function sizeCell(
  totals: Totals,
  groupId: string | undefined,
  display: SizeDisplay,
  text: LocaleText,
): string {
  if (totals.phase === "loading" || totals.phase === "idle")
    return text("ui.browse.loading", "Loading…");
  if (totals.phase === "error")
    return text("ui.groups.size_unknown", "Cannot be confirmed");
  const total =
    groupId === undefined
      ? totals.value.unclassified
      : totals.value.groups.find((entry) => entry.groupId === groupId)?.total;
  return totalSizeText(total, display, text);
}

function decisionCell(row: TableRow, text: LocaleText): string {
  if (row.kind === "unclassified")
    return text("ui.groups.decision_unclassified", "No rule matched");
  if (row.row.ruleUnevaluable)
    return text(
      "ui.groups.decision_unevaluable",
      "Uses a condition that is not registered",
    );
  if (row.row.undecidableCount > 0)
    return text(
      "ui.groups.decision_undecided",
      "{count} files could not be decided",
      { count: String(row.row.undecidableCount) },
    );
  return text("ui.groups.decision_ok", "Decided");
}

function Cell({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="du-groups-cell">
      <div className="du-groups-cell__label" aria-hidden="true">
        {label}
      </div>
      {children}
    </div>
  );
}

/** The marks next to a group name: virtual, shares files with other groups, limited. */
function GroupBadges({ row }: { row: GroupSummaryRow }) {
  const text = useLocaleText();
  return (
    <>
      <StatusBadge tone="info">
        {text("ui.groups.virtual_badge", "Virtual")}
      </StatusBadge>
      {row.overlapMemberCount > 0 && (
        <StatusBadge tone="neutral">
          {text("ui.groups.overlap_badge", "Shares files")}
        </StatusBadge>
      )}
      {isLowerBound(row) && (
        <StatusBadge tone="warning">
          {text("ui.groups.limited_badge", "Limited")}
        </StatusBadge>
      )}
    </>
  );
}

function NameCell({
  row,
  label,
  selected,
  selectable,
  onSelect,
}: {
  row: TableRow;
  label: string;
  selected: boolean;
  selectable: boolean | undefined;
  onSelect: (groupId: string) => void;
}) {
  const text = useLocaleText();
  if (row.kind === "unclassified")
    return (
      <Cell label={label}>
        <Text>{text("ui.groups.tab_unclassified", "Unclassified")}</Text>
      </Cell>
    );
  const { groupId } = row.row;
  const { displayName } = row.row;
  return (
    <Cell label={label}>
      <div className="du-groups-name" data-selected={selected || undefined}>
        {selectable === undefined ? (
          <Text>{displayName}</Text>
        ) : (
          <Action
            variant="quiet"
            isDisabled={!selectable}
            aria-label={text(
              "ui.groups.select_group",
              "Show members of {name}",
              { name: displayName },
            )}
            onPress={() => onSelect(groupId)}
          >
            {displayName}
          </Action>
        )}
        {selectable !== undefined && <GroupBadges row={row.row} />}
        {selected && (
          <div className="du-groups-sr">
            <Text>{text("ui.groups.selected_badge", "Selected")}</Text>
          </div>
        )}
      </div>
    </Cell>
  );
}

/**
 * One row per rule plus the unclassified files, with the size from the group
 * totals. `applied` is the reduced table beside the rule editor: name, members
 * and size of the result that is currently applied.
 */
export function GroupSummaryTable({
  summary,
  totals,
  display,
  selected,
  selectable,
  onSelect,
  applied = false,
}: {
  summary: GroupSummary;
  totals: Totals;
  display: SizeDisplay;
  selected: string | undefined;
  selectable: boolean;
  onSelect: (groupId: string) => void;
  applied?: boolean;
}) {
  const text = useLocaleText();
  const columns = [
    text("ui.groups.col_group", "Name"),
    text("ui.groups.col_members", "Members"),
    text("ui.groups.col_size", "Size"),
    ...(applied
      ? []
      : [
          text("ui.groups.col_overlap", "Overlap with other groups"),
          text("ui.groups.col_decision", "Decision"),
        ]),
  ];
  const rows: TableRow[] = [
    ...summary.groups.map((row) => ({
      kind: "group" as const,
      key: row.groupId,
      row,
    })),
    { kind: "unclassified", key: "unclassified" },
  ];
  return (
    <DataTable<TableRow>
      label={text("ui.groups.table_label", "Virtual group summary")}
      columns={columns}
      columnWidths={applied ? [240, 120, 140] : [420, 150, 150, 190, 210]}
      density="compact"
      rows={rows}
      getRowKey={(row) => row.key}
      renderCells={(row) => {
        const counts =
          row.kind === "group"
            ? rowCountTexts(row.row, text)
            : {
                members: text("ui.groups.items_count", "{count} items", {
                  count: String(summary.unclassifiedCount),
                }),
                overlap: "—",
              };
        const cells = [
          <NameCell
            key="group"
            row={row}
            label={columns[0] ?? ""}
            selected={
              !applied && row.kind === "group" && selected === row.row.groupId
            }
            selectable={applied ? undefined : selectable}
            onSelect={onSelect}
          />,
          <Cell key="members" label={columns[1] ?? ""}>
            <Text>{counts.members}</Text>
          </Cell>,
          <Cell key="size" label={columns[2] ?? ""}>
            <Text>
              {sizeCell(
                totals,
                row.kind === "group" ? row.row.groupId : undefined,
                display,
                text,
              )}
            </Text>
          </Cell>,
        ];
        if (applied) return cells;
        return [
          ...cells,
          <Cell key="overlap" label={columns[3] ?? ""}>
            <Text>{counts.overlap}</Text>
          </Cell>,
          <Cell key="decision" label={columns[4] ?? ""}>
            <Text>{decisionCell(row, text)}</Text>
          </Cell>,
        ];
      }}
    />
  );
}
