import { useEffect, useRef, useState } from "react";
import {
  Action,
  DataTable,
  DetailDisclosure,
  Heading,
  MiddleEllipsisText,
  NameText,
  Notice,
  Progress,
  Readout,
  SelectField,
  Text,
} from "../../components";
import type {
  AnalysisClient,
  HistoryItemOutcome,
  HistoryItemRecording,
  HistoryItemRow,
  HistoryItemsResponse,
} from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import { useHistoryItems } from "../../state/history-items";
import { missingLabel, useHistoryTime } from "./history-labels";
import { observedSizeText, useHistorySizeDisplay } from "./history-size";
import type { SizeDisplay } from "../../i18n/bytes";
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
  // Design #232: an operation without recorded items has nothing to filter.
  // The recorded count does not depend on the filter, so the control stays
  // mounted (and keeps focus) while a filtered page loads.
  const [noItems, setNoItems] = useState(false);
  return (
    <>
      {!noItems && (
        <div className="du-history-item-filter">
          <SelectField
            label={text("ui.history.outcome", "Outcome")}
            value={filter}
            options={filters}
            onChange={setFilter}
          />
        </div>
      )}
      <ItemPages
        key={`${jobRef}:${filter}`}
        client={client}
        jobRef={jobRef}
        outcome={filter === "all" ? null : filter}
        filters={filters.filter((option) => option.value !== "all")}
        onRecorded={(recorded) => setNoItems(recorded === "0")}
      />
    </>
  );
}
function ItemPages({
  client,
  jobRef,
  outcome,
  filters,
  onRecorded,
}: {
  client: AnalysisClient;
  jobRef: string;
  outcome: HistoryItemOutcome | null;
  filters: { value: Filter; label: string }[];
  /** Reports the whole operation's recorded item count once a page is read. */
  onRecorded: (recorded: string) => void;
}) {
  const text = useLocaleText();
  const display = useHistorySizeDisplay();
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
  const recorded = page?.counts.recorded;
  useEffect(() => {
    if (recorded !== undefined) onRecorded(recorded);
  }, [recorded, onRecorded]);
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
          <ItemCounts page={page} />
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
                    display,
                    text,
                  )
                }
              />
            </div>
          )}
          {page.rows.length === 0 && (
            <EmptyItems outcome={outcome} page={page} />
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
  page,
}: {
  outcome: HistoryItemOutcome | null;
  page: HistoryItemsResponse;
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
  const recorded = itemsRecorded(page);
  // The count sentence already says that results are missing.
  if (recorded && page.counts.unavailable !== "0") return null;
  return (
    <Text>
      {recorded
        ? text(
            "ui.history.items_none_processed",
            "No items were processed in this task.",
          )
        : text(
            "ui.history.items_not_recorded",
            "This task didn't record results for each item.",
          )}
    </Text>
  );
}

function ItemCompleteness({ page }: { page: HistoryItemsResponse }) {
  const text = useLocaleText();
  // The count sentence states missing results, so the notice does not repeat it.
  const reasons = page.missingReasons.filter(
    (reason) => reason !== "items_incomplete",
  );
  if (reasons.length === 0 && page.rows.every((row) => row.result !== null))
    return null;
  return (
    <Notice>
      {text(
        "ui.history.item_information_unavailable",
        "Some item information is unavailable.",
      )}{" "}
      {reasons.map((reason) => missingLabel(reason, text)).join(" ")}
    </Notice>
  );
}

/** An older engine omits the value; only item-level history has a summary count. */
function itemsRecorded(page: HistoryItemsResponse): boolean {
  const recording: HistoryItemRecording | undefined = page.itemRecording;
  return (
    recording === "recorded" ||
    (recording !== "not_recorded" && page.counts.summary !== null)
  );
}

/**
 * One sentence on how many item results remain. Nothing is shown when no item
 * was recorded and none is missing; the empty state explains that case.
 */
function ItemCounts({ page }: { page: HistoryItemsResponse }) {
  const text = useLocaleText();
  const { summary, recorded, unavailable } = page.counts;
  if (!itemsRecorded(page) || (recorded === "0" && unavailable === "0"))
    return null;
  const count = BigInt(recorded);
  return (
    <Text>
      {summary === null
        ? text(
            "ui.history.item_count_total_unknown",
            count === 1n
              ? "{recorded} item with results · Total unknown"
              : "{recorded} items with results · Total unknown",
            { recorded: count },
          )
        : unavailable === "0"
          ? text(
              "ui.history.item_count_all",
              count === 1n ? "{recorded} item" : "{recorded} items",
              { recorded: count },
            )
          : text(
              "ui.history.item_count_partial",
              "Results are kept for {recorded} of {summary} items.",
              { recorded: count, summary: BigInt(summary) },
            )}
    </Text>
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
  const notes = itemFlags(result, text);
  const display = useHistorySizeDisplay();
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
            // Design #232: the full path marks edge spaces as the table does.
            value: <NameText name={result.locationDisplay} path />,
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
            value: observedSizeText(result, display, text),
          },
          // Design #232: an item without notes has no notes row.
          ...(notes
            ? [{ label: text("ui.history.notes", "Notes"), value: notes }]
            : []),
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
                  displaced.name === null ? (
                    text("ui.history.unavailable", "Unavailable")
                  ) : (
                    <NameText name={displaced.name} />
                  ),
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
                  "Time moved to Recycle Bin",
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
              "Use the recorded details to check the Recycle Bin yourself.",
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
  display: SizeDisplay,
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
  const size = observedSizeText(result, display, text);
  return [
    // F02 README #170: the end of the path stays; the full path is read by
    // screen readers, shown to a resting pointer and given by "View".
    <div className="du-history-item-location" key="location">
      <MiddleEllipsisText text={result.locationDisplay} cell />
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
