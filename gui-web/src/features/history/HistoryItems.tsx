import { useEffect, useRef, useState } from "react";
import {
  Action,
  DataTable,
  Heading,
  Notice,
  Progress,
  Readout,
  SelectField,
  Text,
} from "../../components";
import type {
  AnalysisClient,
  HistoryItemOutcome,
  HistoryItemRow,
} from "../../ipc";
import { useHistoryItems } from "../../state/history-items";
import { missingLabel } from "./history-labels";
type Filter = HistoryItemOutcome | "all";
const filters: { value: Filter; label: string }[] = [
  { value: "all", label: "All outcomes" },
  { value: "completed", label: "Completed" },
  { value: "overwritten", label: "Overwritten" },
  { value: "skipped", label: "Skipped" },
  { value: "failed", label: "Failed" },
  { value: "unprocessed", label: "Unprocessed" },
];
export function HistoryItemSection({
  client,
  jobRef,
}: {
  client: AnalysisClient;
  jobRef: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <section className="du-results" aria-label="Item results">
      <Heading level={3}>Item results</Heading>
      <Action onPress={() => setOpen(!open)}>
        {open ? "Hide item results" : "View item results"}
      </Action>
      {open && <HistoryItems client={client} jobRef={jobRef} />}
    </section>
  );
}
export function HistoryItems({
  client,
  jobRef,
}: {
  client: AnalysisClient;
  jobRef: string;
}) {
  const [filter, setFilter] = useState<Filter>("all");
  return (
    <>
      <SelectField
        label="Outcome"
        value={filter}
        options={filters}
        onChange={setFilter}
      />
      <ItemPages
        key={`${jobRef}:${filter}`}
        client={client}
        jobRef={jobRef}
        outcome={filter === "all" ? null : filter}
      />
    </>
  );
}
function ItemPages({
  client,
  jobRef,
  outcome,
}: {
  client: AnalysisClient;
  jobRef: string;
  outcome: HistoryItemOutcome | null;
}) {
  const { page, pending, error, refreshed, more, retry } = useHistoryItems(
    client,
    jobRef,
    outcome,
  );
  const [selected, setSelected] = useState<HistoryItemRow>();
  const displaced =
    selected && page?.rows.includes(selected)
      ? selected.result?.displaced
      : undefined;
  return (
    <>
      {refreshed && (
        <Notice>
          History changed. Item results were restarted from the first page.
        </Notice>
      )}
      {pending && <Progress label="Loading item results" />}
      {error === "removed" && (
        <Notice>History was removed and can no longer be viewed.</Notice>
      )}
      {error === "storage" && (
        <Notice tone="error">
          Item results could not be loaded.{" "}
          <Action onPress={retry} isDisabled={pending}>
            Retry
          </Action>
        </Notice>
      )}
      {page && (
        <>
          {(page.completeness === "partial" ||
            page.missingReasons.length > 0 ||
            page.rows.some((row) => row.result === null)) && (
            <Notice>
              Some item information is unavailable.{" "}
              {page.missingReasons.map(missingLabel).join(" ")}
            </Notice>
          )}
          <Readout
            label="Item counts"
            rows={[
              {
                label: "Summary count",
                value: page.counts.summary ?? "Unavailable",
              },
              { label: "Recorded count", value: page.counts.recorded },
              {
                label: "Unavailable count",
                value: page.counts.unavailable ?? "Unavailable",
              },
            ]}
          />
          <DataTable
            label="Item results"
            columns={["Item", "Recorded location", "Outcome", "Details"]}
            columnWidths={[168, 258, 110, 254]}
            rows={page.rows}
            renderCells={(row) => cells(row, setSelected)}
          />
          {page.rows.length === 0 && (
            <EmptyItems outcome={outcome} summary={page.counts.summary} />
          )}
          {page.nextCursor && (
            <Action onPress={more} isDisabled={pending}>
              Load more items
            </Action>
          )}
        </>
      )}
      {displaced && (
        <DisplacedDetails key={selected?.itemSeq} displaced={displaced} />
      )}
    </>
  );
}
function EmptyItems({
  outcome,
  summary,
}: {
  outcome: HistoryItemOutcome | null;
  summary: string | null;
}) {
  if (outcome !== null)
    return <Text>No recorded items match this outcome.</Text>;
  return (
    <Text>
      {summary === null
        ? "No item records were returned; the summary count is unavailable."
        : "No recorded items were returned."}
    </Text>
  );
}

function DisplacedDetails({
  displaced,
}: {
  displaced: NonNullable<NonNullable<HistoryItemRow["result"]>["displaced"]>;
}) {
  const region = useRef<HTMLDivElement>(null);
  useEffect(() => {
    region.current?.focus();
  }, [displaced]);
  return (
    <div
      ref={region}
      tabIndex={-1}
      role="region"
      aria-label="Displaced item details"
    >
      <Heading level={3}>Displaced item details</Heading>
      <Readout
        label="Displaced item details"
        rows={[
          { label: "Name", value: displaced.name ?? "Unavailable" },
          {
            label: "Original recorded location",
            value: displaced.originalLocationDisplay ?? "Unavailable",
          },
          {
            label: "Time moved to trash",
            value: displayTime(displaced.trashedAtMs),
          },
        ]}
      />
      {displaced.ambiguous && (
        <Notice>The displayed information is ambiguous.</Notice>
      )}
      <Notice>
        Check the operating system trash directly using the recorded
        information.
      </Notice>
    </div>
  );
}

function cells(row: HistoryItemRow, select: (row: HistoryItemRow) => void) {
  const result = row.result;
  if (!result)
    return [
      `Item ${row.itemSeq}`,
      "Unavailable",
      "Unavailable",
      "Item result unavailable",
    ];
  const flags = [
    result.reviewNeeded && "Review needed",
    result.held && "Held",
    result.outcomeUnknown && "Outcome unknown",
    result.reasonKey && "A reason was recorded",
  ]
    .filter(Boolean)
    .join(" · ");
  const size =
    result.observedSize === null || result.observedSizeState === "unknown"
      ? "Size unavailable"
      : result.observedSizeState === "exact"
        ? `${result.observedSize} bytes`
        : result.observedSizeState === "lower_bound"
          ? `${result.observedSize} bytes or more`
          : "Size unavailable";
  return [
    <div key="item">
      <Text>Item {row.itemSeq}</Text>
      {result.displaced && (
        <Action
          aria-label={`View displaced item details, item ${row.itemSeq}`}
          onPress={() => select(row)}
        >
          Displacement details
        </Action>
      )}
    </div>,
    result.locationDisplay,
    filters.find((filter) => filter.value === result.outcome)?.label ??
      "Outcome unavailable",
    `${size}${flags ? ` · ${flags}` : ""}`,
  ];
}
function displayTime(value: number | null) {
  if (value === null) return "Unavailable";
  const date = new Date(value);
  return Number.isFinite(date.getTime())
    ? date.toLocaleString()
    : "Unavailable";
}
