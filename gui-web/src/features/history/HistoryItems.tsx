import { useEffect, useRef, useState } from "react";
import {
  Action,
  DataTable,
  DetailDisclosure,
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
  HistoryItemsResponse,
} from "../../ipc";
import { useHistoryItems } from "../../state/history-items";
import { missingLabel, useHistoryTime } from "./history-labels";
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
  return (
    <DetailDisclosure title="Item results">
      <HistoryItems client={client} jobRef={jobRef} />
    </DetailDisclosure>
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
      <div className="du-history-item-filter">
        <SelectField
          label="Outcome"
          value={filter}
          options={filters}
          onChange={setFilter}
        />
      </div>
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
  const opener = useRef<HTMLElement>(null);
  const table = useRef<HTMLDivElement>(null);
  const selectedResult =
    selected && page?.rows.includes(selected) ? selected.result : undefined;
  return (
    <div className="du-history-item-results">
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
          <ItemCompleteness page={page} />
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
          {page.rows.length > 0 && (
            <div ref={table}>
              <DataTable
                label="Item results"
                columns={[
                  "Recorded location",
                  "Outcome",
                  "Observed size and notes",
                  "Details",
                ]}
                columnWidths={[157, 102, 136, 106]}
                density="compact"
                totalRows={page.nextCursor ? -1 : page.rows.length}
                rows={page.rows}
                getRowKey={(row) => row.itemSeq}
                renderCells={(row) =>
                  cells(row, (target) => {
                    opener.current = target;
                    setSelected(row);
                  })
                }
              />
            </div>
          )}
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
      {selectedResult && (
        <RecordedItemDetails
          key={selected?.itemSeq}
          result={selectedResult}
          onClose={() => {
            setSelected(undefined);
            const target = opener.current?.isConnected
              ? opener.current
              : table.current?.querySelector<HTMLElement>(".du-table-viewport");
            target?.focus();
          }}
        />
      )}
    </div>
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

function ItemCompleteness({ page }: { page: HistoryItemsResponse }) {
  if (
    page.completeness === "complete" &&
    page.missingReasons.length === 0 &&
    page.rows.every((row) => row.result !== null)
  )
    return null;
  return (
    <Notice>
      Some item information is unavailable.{" "}
      {page.missingReasons.map(missingLabel).join(" ")}
    </Notice>
  );
}

function RecordedItemDetails({
  result,
  onClose,
}: {
  result: NonNullable<HistoryItemRow["result"]>;
  onClose: () => void;
}) {
  const displaced = result.displaced;
  const displayTime = useHistoryTime();
  const title = displaced ? "Displaced item details" : "Recorded item details";
  const region = useRef<HTMLDivElement>(null);
  useEffect(() => {
    region.current?.focus({ preventScroll: true });
    region.current?.scrollIntoView({ block: "start", inline: "nearest" });
  }, [result]);
  return (
    <div
      ref={region}
      className="du-history-displaced"
      tabIndex={-1}
      role="region"
      aria-label={title}
    >
      <div className="du-history-header">
        <Heading level={3}>{title}</Heading>
        <Action variant="quiet" onPress={onClose}>
          Close details
        </Action>
      </div>
      <Readout
        label="Recorded item"
        rows={[
          { label: "Recorded location", value: result.locationDisplay },
          {
            label: "Outcome",
            value:
              filters.find((filter) => filter.value === result.outcome)
                ?.label ?? "Outcome unavailable",
          },
          { label: "Observed size", value: observedSize(result) },
          { label: "Notes", value: itemFlags(result) || "No additional notes" },
        ]}
      />
      {displaced && (
        <>
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
                value: displayTime(displaced.trashedAtMs, "Unavailable"),
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
        </>
      )}
    </div>
  );
}

function cells(row: HistoryItemRow, select: (target: HTMLElement) => void) {
  const result = row.result;
  if (!result)
    return [
      "Item " + row.itemSeq,
      "Unavailable",
      "Item result unavailable",
      null,
    ];
  const flags = itemFlags(result);
  const size = observedSize(result);
  return [
    <div
      className="du-history-item-location"
      key="location"
      title={result.locationDisplay}
    >
      <Text>{result.locationDisplay}</Text>
    </div>,
    filters.find((filter) => filter.value === result.outcome)?.label ??
      "Outcome unavailable",
    <div className="du-history-item-observation" key="observation">
      <Text>{size}</Text>
      {flags && <Text kind="muted">{flags}</Text>}
    </div>,
    <Action
      key="displacement"
      variant="quiet"
      aria-label={
        (result.displaced
          ? "View displaced item details, item "
          : "View item result, item ") + row.itemSeq
      }
      onPress={(event) => {
        if (event.target instanceof HTMLElement) select(event.target);
      }}
    >
      View item
    </Action>,
  ];
}

function itemFlags(result: NonNullable<HistoryItemRow["result"]>) {
  return [
    result.reviewNeeded && "Review needed",
    result.held && "Held",
    result.outcomeUnknown && "Outcome unknown",
    result.reasonKey && "A reason was recorded",
  ]
    .filter(Boolean)
    .join(" · ");
}

function observedSize(result: NonNullable<HistoryItemRow["result"]>) {
  if (result.observedSize === null) return "Size unavailable";
  if (result.observedSizeState === "exact")
    return result.observedSize + " bytes";
  if (result.observedSizeState === "lower_bound")
    return result.observedSize + " bytes or more";
  return "Size unavailable";
}
