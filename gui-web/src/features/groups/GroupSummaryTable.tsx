import { Action, DataTable, StatusBadge, Text } from "../../components";
import type { SizeDisplay } from "../../i18n/bytes";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { GroupSummary, GroupSummaryRow, GroupTotals } from "../../ipc";
import type { ReadState, TotalsKey } from "../../state/groups-details";
import { sizeValueText } from "./group-format";
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
    return text("ui.groups.size_unknown", "Unknown");
  const total =
    groupId === undefined
      ? totals.value.unclassified
      : totals.value.groups.find((entry) => entry.groupId === groupId)?.total;
  const entry =
    total?.sizes.find((size) => size.size.basis === "logical") ??
    total?.sizes[0];
  return entry
    ? sizeValueText(entry.size, display, text)
    : text("ui.groups.size_unknown", "Unknown");
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

/** One row per rule plus the unclassified files, with the size from the group totals. */
export function GroupSummaryTable({
  summary,
  totals,
  display,
  selected,
  selectable,
  onSelect,
}: {
  summary: GroupSummary;
  totals: Totals;
  display: SizeDisplay;
  selected: string | undefined;
  selectable: boolean;
  onSelect: (groupId: string) => void;
}) {
  const text = useLocaleText();
  const columns = [
    text("ui.groups.col_group", "Virtual group"),
    text("ui.groups.col_members", "Members"),
    text("ui.groups.col_size", "Size"),
    text("ui.groups.col_overlap", "In several groups"),
    text("ui.groups.col_decision", "Decision"),
  ];
  const rows: TableRow[] = [
    ...summary.groups.map((row) => ({
      kind: "group" as const,
      key: row.groupId,
      row,
    })),
    { kind: "unclassified", key: "unclassified" },
  ];
  const count = (value: number) =>
    text("ui.groups.items_count", "{count} items", { count: String(value) });
  return (
    <DataTable<TableRow>
      label={text("ui.groups.table_label", "Virtual group summary")}
      columns={columns}
      columnWidths={[200, 110, 130, 130, 200]}
      density="compact"
      rows={rows}
      getRowKey={(row) => row.key}
      renderCells={(row) => [
        <Cell key="group" label={columns[0] ?? ""}>
          {row.kind === "group" ? (
            <div className="du-groups-name">
              <Action
                variant="quiet"
                isDisabled={!selectable}
                aria-label={text(
                  "ui.groups.select_group",
                  "Show members of {name}",
                  { name: row.row.displayName },
                )}
                onPress={() => onSelect(row.row.groupId)}
              >
                {row.row.displayName}
              </Action>
              <StatusBadge tone="info">
                {text("ui.groups.virtual_badge", "Virtual group")}
              </StatusBadge>
              {selectable && selected === row.row.groupId && (
                <StatusBadge tone="neutral">
                  {text("ui.groups.selected_badge", "Selected")}
                </StatusBadge>
              )}
            </div>
          ) : (
            <Text>{text("ui.groups.tab_unclassified", "Unclassified")}</Text>
          )}
        </Cell>,
        <Cell key="members" label={columns[1] ?? ""}>
          <Text>
            {count(
              row.kind === "group"
                ? row.row.memberCount
                : summary.unclassifiedCount,
            )}
          </Text>
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
        <Cell key="overlap" label={columns[3] ?? ""}>
          <Text>
            {row.kind === "group" ? count(row.row.overlapMemberCount) : "—"}
          </Text>
        </Cell>,
        <Cell key="decision" label={columns[4] ?? ""}>
          <Text>{decisionCell(row, text)}</Text>
        </Cell>,
      ]}
    />
  );
}
