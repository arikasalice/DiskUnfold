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
import { useLocaleText } from "../../i18n/presentation";
import { useHistoryItems } from "../../state/history-items";
import { missingLabel, useHistoryTime } from "./history-labels";
type Filter = HistoryItemOutcome | "all";
export function HistoryItemSection({
  client,
  jobRef,
}: {
  client: AnalysisClient;
  jobRef: string;
}) {
  const text = useLocaleText();
  return (
    <DetailDisclosure title={text("ui.history.item_results", "Item results")}>
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
  const text = useLocaleText();
  const filters: { value: Filter; label: string }[] = [
    { value: "all", label: text("ui.history.all_outcomes", "All outcomes") },
    {
      value: "completed",
      label: text("ui.history.outcome_completed", "Completed"),
    },
    {
      value: "overwritten",
      label: text("ui.history.outcome_overwritten", "Overwritten"),
    },
    { value: "skipped", label: text("ui.history.outcome_skipped", "Skipped") },
    { value: "failed", label: text("ui.history.outcome_failed", "Failed") },
    {
      value: "unprocessed",
      label: text("ui.history.outcome_unprocessed", "Unprocessed"),
    },
  ];
  const [filter, setFilter] = useState<Filter>("all");
  return (
    <>
      <div className="du-history-item-filter">
        <SelectField
          label={text("ui.history.outcome", "Outcome")}
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
        filters={filters.filter((option) => option.value !== "all")}
      />
    </>
  );
}
function ItemPages({
  client,
  jobRef,
  outcome,
  filters,
}: {
  client: AnalysisClient;
  jobRef: string;
  outcome: HistoryItemOutcome | null;
  filters: { value: Filter; label: string }[];
}) {
  const text = useLocaleText();
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
          {text(
            "ui.history.items_changed_restart_page",
            "History changed. Item results were restarted from the first page.",
          )}
        </Notice>
      )}
      {pending && (
        <Progress
          label={text("ui.history.loading_items", "Loading item results")}
        />
      )}
      {error === "removed" && (
        <Notice>
          {text(
            "ui.history.removed",
            "History was removed and can no longer be viewed.",
          )}
        </Notice>
      )}
      {error === "storage" && (
        <Notice tone="error">
          {text(
            "ui.history.items_load_failed",
            "Item results could not be loaded.",
          )}{" "}
          <Action onPress={retry} isDisabled={pending}>
            {text("ui.history.retry", "Retry")}
          </Action>
        </Notice>
      )}
      {page && (
        <>
          <ItemCompleteness page={page} />
          <Readout
            label={text("ui.history.item_counts", "Item counts")}
            rows={[
              {
                label: text("ui.history.summary_count", "Summary count"),
                value:
                  page.counts.summary ??
                  text("ui.history.unavailable", "Unavailable"),
              },
              {
                label: text("ui.history.recorded_count", "Recorded count"),
                value: page.counts.recorded,
              },
              {
                label: text(
                  "ui.history.unavailable_count",
                  "Unavailable count",
                ),
                value:
                  page.counts.unavailable ??
                  text("ui.history.unavailable", "Unavailable"),
              },
            ]}
          />
          {page.rows.length > 0 && (
            <div ref={table}>
              <DataTable
                label={text("ui.history.item_results", "Item results")}
                columns={[
                  text("ui.history.recorded_location", "Recorded location"),
                  text("ui.history.outcome", "Outcome"),
                  text(
                    "ui.history.observed_size_notes",
                    "Observed size and notes",
                  ),
                  text("ui.history.details", "Details"),
                ]}
                columnWidths={[157, 102, 136, 106]}
                density="compact"
                totalRows={page.nextCursor ? -1 : page.rows.length}
                rows={page.rows}
                getRowKey={(row) => row.itemSeq}
                renderCells={(row) =>
                  cells(
                    row,
                    (target) => {
                      opener.current = target;
                      setSelected(row);
                    },
                    filters,
                    text,
                  )
                }
              />
            </div>
          )}
          {page.rows.length === 0 && (
            <EmptyItems outcome={outcome} summary={page.counts.summary} />
          )}
          {page.nextCursor && (
            <Action onPress={more} isDisabled={pending}>
              {text("ui.history.load_more_items", "Load more items")}
            </Action>
          )}
        </>
      )}
      {selectedResult && (
        <RecordedItemDetails
          key={selected?.itemSeq}
          result={selectedResult}
          filters={filters}
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
  const text = useLocaleText();
  if (outcome !== null)
    return (
      <Text>
        {text(
          "ui.history.no_items_match",
          "No recorded items match this outcome.",
        )}
      </Text>
    );
  return (
    <Text>
      {summary === null
        ? text(
            "ui.history.no_item_records_no_summary",
            "No item records were returned; the summary count is unavailable.",
          )
        : text(
            "ui.history.no_item_records",
            "No recorded items were returned.",
          )}
    </Text>
  );
}

function ItemCompleteness({ page }: { page: HistoryItemsResponse }) {
  const text = useLocaleText();
  if (
    page.completeness === "complete" &&
    page.missingReasons.length === 0 &&
    page.rows.every((row) => row.result !== null)
  )
    return null;
  return (
    <Notice>
      {text(
        "ui.history.item_information_unavailable",
        "Some item information is unavailable.",
      )}{" "}
      {page.missingReasons
        .map((reason) => missingLabel(reason, text))
        .join(" ")}
    </Notice>
  );
}

function RecordedItemDetails({
  result,
  filters,
  onClose,
}: {
  result: NonNullable<HistoryItemRow["result"]>;
  filters: { value: Filter; label: string }[];
  onClose: () => void;
}) {
  const text = useLocaleText();
  const displaced = result.displaced;
  const displayTime = useHistoryTime();
  const title = displaced
    ? text("ui.history.displaced_item_details", "Displaced item details")
    : text("ui.history.recorded_item_details", "Recorded item details");
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
          {text("ui.history.close_details", "Close details")}
        </Action>
      </div>
      <Readout
        label={text("ui.history.recorded_item", "Recorded item")}
        rows={[
          {
            label: text("ui.history.recorded_location", "Recorded location"),
            value: result.locationDisplay,
          },
          {
            label: text("ui.history.outcome", "Outcome"),
            value:
              filters.find((filter) => filter.value === result.outcome)
                ?.label ??
              text("ui.history.outcome_unavailable", "Outcome unavailable"),
          },
          {
            label: text("ui.history.observed_size", "Observed size"),
            value: observedSize(result, text),
          },
          {
            label: text("ui.history.notes", "Notes"),
            value:
              itemFlags(result, text) ||
              text("ui.history.no_additional_notes", "No additional notes"),
          },
        ]}
      />
      {displaced && (
        <>
          <Readout
            label={text(
              "ui.history.displaced_item_details",
              "Displaced item details",
            )}
            rows={[
              {
                label: text("ui.history.name", "Name"),
                value:
                  displaced.name ??
                  text("ui.history.unavailable", "Unavailable"),
              },
              {
                label: text(
                  "ui.history.original_recorded_location",
                  "Original recorded location",
                ),
                value:
                  displaced.originalLocationDisplay ??
                  text("ui.history.unavailable", "Unavailable"),
              },
              {
                label: text(
                  "ui.history.time_moved_to_trash",
                  "Time moved to trash",
                ),
                value: displayTime(
                  displaced.trashedAtMs,
                  text("ui.history.unavailable", "Unavailable"),
                ),
              },
            ]}
          />
          {displaced.ambiguous && (
            <Notice>
              {text(
                "ui.history.ambiguous_information",
                "The displayed information is ambiguous.",
              )}
            </Notice>
          )}
          <Notice>
            {text(
              "ui.history.check_os_trash",
              "Check the operating system trash directly using the recorded information.",
            )}
          </Notice>
        </>
      )}
    </div>
  );
}

function cells(
  row: HistoryItemRow,
  select: (target: HTMLElement) => void,
  filters: { value: Filter; label: string }[],
  text: ReturnType<typeof useLocaleText>,
) {
  const result = row.result;
  if (!result)
    return [
      text("ui.history.item_number", "Item {number}", { number: row.itemSeq }),
      text("ui.history.unavailable", "Unavailable"),
      text("ui.history.item_result_unavailable", "Item result unavailable"),
      null,
    ];
  const flags = itemFlags(result, text);
  const size = observedSize(result, text);
  return [
    <div
      className="du-history-item-location"
      key="location"
      title={result.locationDisplay}
    >
      <Text>{result.locationDisplay}</Text>
    </div>,
    filters.find((filter) => filter.value === result.outcome)?.label ??
      text("ui.history.outcome_unavailable", "Outcome unavailable"),
    <div className="du-history-item-observation" key="observation">
      <Text>{size}</Text>
      {flags && <Text kind="muted">{flags}</Text>}
    </div>,
    <Action
      key="displacement"
      variant="quiet"
      aria-label={text(
        result.displaced
          ? "ui.history.view_displaced_item_aria"
          : "ui.history.view_item_aria",
        result.displaced
          ? "View displaced item details, item {number}"
          : "View item result, item {number}",
        { number: row.itemSeq },
      )}
      onPress={(event) => {
        if (event.target instanceof HTMLElement) select(event.target);
      }}
    >
      {text("ui.history.view_item", "View item")}
    </Action>,
  ];
}

function itemFlags(
  result: NonNullable<HistoryItemRow["result"]>,
  text: ReturnType<typeof useLocaleText>,
) {
  return [
    result.reviewNeeded && text("ui.history.review_needed", "Review needed"),
    result.held && text("ui.history.held", "Held"),
    result.outcomeUnknown &&
      text("ui.history.outcome_unknown", "Outcome unknown"),
    result.reasonKey &&
      text("ui.history.reason_recorded", "A reason was recorded"),
  ]
    .filter(Boolean)
    .join(" · ");
}

function observedSize(
  result: NonNullable<HistoryItemRow["result"]>,
  text: ReturnType<typeof useLocaleText>,
) {
  if (result.observedSize === null)
    return text("ui.history.size_unavailable", "Size unavailable");
  if (result.observedSizeState === "exact")
    return result.observedSize + " " + text("ui.history.bytes", "bytes");
  if (result.observedSizeState === "lower_bound")
    return (
      result.observedSize +
      " " +
      text("ui.history.bytes_or_more", "bytes or more")
    );
  return text("ui.history.size_unavailable", "Size unavailable");
}
