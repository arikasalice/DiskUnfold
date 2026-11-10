import { useMemo, useState } from "react";
import {
  Action,
  DataTable,
  DateField,
  Heading,
  Notice,
  Progress,
  Readout,
  SelectField,
  Text,
} from "../../components";
import type {
  AnalysisClient,
  HistoryEndState,
  HistoryListFilter,
  HistoryListRow,
} from "../../ipc";
import { useHistory } from "../../state/history";

const states = [
  { value: "all", label: "All states" },
  { value: "done", label: "Completed" },
  { value: "partial", label: "Partially completed" },
  { value: "failed", label: "Failed" },
  { value: "cancelled", label: "Cancelled" },
  { value: "interrupted", label: "Interrupted" },
] as const;
const kinds = [
  { value: "all", label: "All types" },
  { value: "scan.metadata", label: "Disk analysis" },
] as const;
type StateFilter = (typeof states)[number]["value"];
type KindFilter = (typeof kinds)[number]["value"];

function day(value: string, next: boolean): number | null {
  if (!value) return null;
  const date = new Date(`${value}T00:00:00`);
  if (next) date.setDate(date.getDate() + 1);
  const result = date.getTime();
  return Number.isFinite(result) ? result : null;
}

/** A separate, read-only view of paged job history. */
export function HistoryPanel({
  client,
  onClose,
}: {
  client: AnalysisClient;
  onClose: () => void;
}) {
  const [from, setFrom] = useState("");
  const [through, setThrough] = useState("");
  const [kind, setKind] = useState<KindFilter>("all");
  const [state, setState] = useState<StateFilter>("all");
  const filter = useMemo<HistoryListFilter>(
    () => ({
      finishedFromMs: day(from, false),
      finishedBeforeMs: day(through, true),
      kinds: kind === "all" ? [] : [kind],
      states: state === "all" ? [] : [state as HistoryEndState],
      relations: [],
    }),
    [from, through, kind, state],
  );
  const invalid =
    filter.finishedFromMs !== null &&
    filter.finishedBeforeMs !== null &&
    filter.finishedFromMs >= filter.finishedBeforeMs;
  return (
    <section className="du-results" aria-labelledby="history-heading">
      <div className="du-card__header">
        <Heading level={2} id="history-heading">
          Operation history
        </Heading>
        <Action autoFocus onPress={onClose}>
          Back to analysis
        </Action>
      </div>
      <div className="du-card__actions">
        <DateField label="From date" value={from} onChange={setFrom} />
        <DateField label="Through date" value={through} onChange={setThrough} />
        <SelectField
          label="Job type"
          value={kind}
          options={kinds}
          onChange={setKind}
        />
        <SelectField
          label="End state"
          value={state}
          options={states}
          onChange={setState}
        />
      </div>
      {invalid ? (
        <Notice tone="error">Choose a valid date range.</Notice>
      ) : (
        <HistoryPages client={client} filter={filter} />
      )}
    </section>
  );
}

function HistoryPages({
  client,
  filter,
}: {
  client: AnalysisClient;
  filter: HistoryListFilter;
}) {
  const { page, pending, error, refreshed, retry, more } = useHistory(
    client,
    filter,
  );
  return (
    <>
      {refreshed && (
        <Notice>History changed. Showing the first page again.</Notice>
      )}
      {pending && <Progress label="Loading history" />}
      {error && (
        <Notice tone="error">
          History could not be loaded. <Action onPress={retry}>Retry</Action>
        </Notice>
      )}
      {!pending && !error && page?.rows.length === 0 && (
        <Text>No history matches these filters.</Text>
      )}
      {page && page.rows.length > 0 && (
        <DataTable
          label="Operation history"
          columns={["Job", "Scope", "State", "Counts", "Finished", "Details"]}
          columnWidths={[95, 100, 100, 240, 110, 145]}
          rows={page.rows}
          renderCells={rowCells}
        />
      )}
      {page?.nextCursor && (
        <Action onPress={more} isDisabled={pending}>
          Load more history
        </Action>
      )}
    </>
  );
}

function rowCells(row: HistoryListRow) {
  const summary = row.summary;
  if (!summary)
    return [
      "Unavailable",
      "Unavailable",
      "Unavailable",
      "Unavailable",
      "Unavailable",
      "Some history could not be read",
    ];
  const counts = summary.counts;
  const date =
    summary.finishedAtMs === null ? null : new Date(summary.finishedAtMs);
  const time =
    date && Number.isFinite(date.getTime())
      ? date.toLocaleString()
      : "Unavailable";
  return [
    summary.kindId === "scan.metadata" ? "Disk analysis" : summary.kindId,
    summary.scopeSummary ?? "Unavailable",
    states.find(({ value }) => value === summary.state)?.label ?? "Unavailable",
    counts ? (
      <Readout
        label="Result counts"
        rows={[
          { label: "Completed", value: counts.completed },
          { label: "Overwritten", value: counts.overwritten },
          { label: "Failed", value: counts.failed },
          { label: "Skipped", value: counts.skipped },
          { label: "Unprocessed", value: counts.unprocessed },
        ]}
      />
    ) : (
      "Unavailable"
    ),
    time,
    [
      summary.relation ? `Follow-up: ${summary.relation}` : "",
      summary.sessionRestoreAvailable === null
        ? "Restore information unavailable"
        : summary.sessionRestoreAvailable
          ? "Restore information available in this session"
          : "No restore information in this session",
      row.missingReasons.length ? "Incomplete" : "",
    ]
      .filter(Boolean)
      .join(" · "),
  ];
}
