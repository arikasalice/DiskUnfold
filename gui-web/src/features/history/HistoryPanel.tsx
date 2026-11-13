import { useEffect, useMemo, useRef, useState } from "react";
import {
  Action,
  DataTable,
  DateField,
  DetailDisclosure,
  Heading,
  LiveStatus,
  Notice,
  Progress,
  SelectField,
  StatusBadge,
  Text,
} from "../../components";
import type {
  AnalysisClient,
  HistoryListFilter,
  HistoryListRow,
} from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import { useHistory } from "../../state/history";
import {
  useHistoryTime,
  kindLabel,
  missingLabel,
  stateLabel,
  stateTone,
} from "./history-labels";
import { HistoryDetailPanel } from "./HistoryDetailPanel";
import "../../theme/history.css";

type StateFilter =
  "all" | "done" | "partial" | "failed" | "cancelled" | "interrupted";
type KindFilter = "all" | "scan.metadata";

function day(value: string, next: boolean): number | null {
  if (!value) return null;
  const date = new Date(value + "T00:00:00");
  if (next) date.setDate(date.getDate() + 1);
  const result = date.getTime();
  return Number.isFinite(result) ? result : null;
}

export function HistoryPanel({
  client,
  onClose,
}: {
  client: AnalysisClient;
  onClose: () => void;
}) {
  const text = useLocaleText();
  const states = [
    { value: "all", label: text("ui.history.filter_all_states", "All states") },
    { value: "done", label: text("ui.history.state_completed", "Completed") },
    {
      value: "partial",
      label: text("ui.history.state_partial", "Partially completed"),
    },
    { value: "failed", label: text("ui.history.state_failed", "Failed") },
    {
      value: "cancelled",
      label: text("ui.history.state_cancelled", "Cancelled"),
    },
    {
      value: "interrupted",
      label: text("ui.history.state_interrupted_filter", "Interrupted"),
    },
  ] as const;
  const kinds = [
    { value: "all", label: text("ui.history.filter_all_types", "All types") },
    {
      value: "scan.metadata",
      label: text("ui.history.kind_disk_analysis", "Disk analysis"),
    },
  ] as const;
  const [from, setFrom] = useState("");
  const [through, setThrough] = useState("");
  const [kind, setKind] = useState<KindFilter>("all");
  const [state, setState] = useState<StateFilter>("all");
  const [selected, setSelected] = useState<string>();
  const opener = useRef<HTMLElement | null>(null);
  const openerLabel = useRef<string | null>(null);
  const back = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const restore = useRef(false);

  useEffect(() => {
    if (selected || !restore.current) return;
    const target = opener.current;
    if (
      target?.isConnected &&
      target.getAttribute("aria-label") === openerLabel.current
    )
      target.focus();
    else
      (
        list.current?.querySelector<HTMLElement>(".du-table-viewport") ??
        back.current
      )?.focus();
    restore.current = false;
  }, [selected]);

  const filter = useMemo<HistoryListFilter>(
    () => ({
      finishedFromMs: day(from, false),
      finishedBeforeMs: day(through, true),
      kinds: kind === "all" ? [] : [kind],
      states: state === "all" ? [] : [state],
      relations: [],
    }),
    [from, through, kind, state],
  );
  const invalid =
    filter.finishedFromMs !== null &&
    filter.finishedBeforeMs !== null &&
    filter.finishedFromMs >= filter.finishedBeforeMs;
  const extraFilters = Boolean(from || through || kind !== "all");
  const hasFilters = extraFilters || state !== "all";

  return (
    <section
      className="du-history-shell"
      aria-label={text("ui.history.operation_history", "Operation history")}
    >
      <div ref={list} className="du-history-screen" hidden={Boolean(selected)}>
        <header className="du-history-header">
          <Heading level={2}>
            {text("ui.history.operation_history", "Operation history")}
          </Heading>
          <Action
            variant="quiet"
            icon="back"
            ref={back}
            autoFocus
            onPress={onClose}
          >
            {text("ui.history.back_to_analysis", "Back to analysis")}
          </Action>
        </header>
        <div className="du-history-controls">
          <div className="du-history-filter-row">
            <SelectField
              label={text("ui.history.end_state", "End state")}
              value={state}
              options={states}
              onChange={setState}
            />
            {hasFilters && (
              <Action
                variant="quiet"
                onPress={() => {
                  setFrom("");
                  setThrough("");
                  setKind("all");
                  setState("all");
                  list.current
                    ?.querySelector<HTMLSelectElement>(
                      ".du-history-filter-row select",
                    )
                    ?.focus();
                }}
              >
                {text("ui.history.clear_filters", "Clear filters")}
              </Action>
            )}
          </div>
          <DetailDisclosure
            title={
              extraFilters
                ? text(
                    "ui.history.date_type_filters_active",
                    "Date and type filters (active)",
                  )
                : text("ui.history.date_type_filters", "Date and type filters")
            }
          >
            <div className="du-history-extra-filters">
              <DateField
                label={text("ui.history.from_date", "From date")}
                value={from}
                onChange={setFrom}
              />
              <DateField
                label={text("ui.history.through_date", "Through date")}
                value={through}
                onChange={setThrough}
              />
              <SelectField
                label={text("ui.history.job_type", "Job type")}
                value={kind}
                options={kinds}
                onChange={setKind}
              />
            </div>
          </DetailDisclosure>
        </div>
        {invalid ? (
          <Notice tone="error">
            {text(
              "ui.history.invalid_date_range",
              "Choose a valid date range.",
            )}
          </Notice>
        ) : (
          <HistoryPages
            client={client}
            filter={filter}
            hasFilters={hasFilters}
            onOpen={(job, target) => {
              opener.current = target;
              openerLabel.current = target.getAttribute("aria-label");
              setSelected(job);
            }}
          />
        )}
      </div>
      {selected && (
        <HistoryDetailPanel
          key={selected}
          client={client}
          jobRef={selected}
          onNavigate={setSelected}
          onClose={() => {
            restore.current = true;
            setSelected(undefined);
          }}
        />
      )}
    </section>
  );
}

function HistoryPages({
  client,
  filter,
  hasFilters,
  onOpen,
}: {
  client: AnalysisClient;
  filter: HistoryListFilter;
  hasFilters: boolean;
  onOpen: (jobRef: string, target: HTMLElement) => void;
}) {
  const text = useLocaleText();
  const { page, pending, error, refreshed, retry, more } = useHistory(
    client,
    filter,
  );
  const displayTime = useHistoryTime();
  const table = useRef<HTMLDivElement>(null);
  const pagingFocused = useRef(false);
  const hasMore = Boolean(page?.nextCursor);
  const shown = page?.rows.length ?? 0;

  useEffect(() => {
    if (!hasMore && !pending && pagingFocused.current) {
      if (document.activeElement === document.body)
        table.current
          ?.querySelector<HTMLElement>(".du-table-viewport")
          ?.focus();
      pagingFocused.current = false;
    }
  }, [hasMore, pending]);

  return (
    <div className="du-history-pages">
      {refreshed && (
        <Notice>
          {text(
            "ui.history.changed_restart_page",
            "History changed. Showing the first page again.",
          )}
        </Notice>
      )}
      {pending && (
        <Progress label={text("ui.history.loading", "Loading history")} />
      )}
      {error && (
        <Notice tone="error">
          {text("ui.history.load_failed", "History could not be loaded.")}{" "}
          <Action onPress={retry}>{text("ui.history.retry", "Retry")}</Action>
        </Notice>
      )}
      {!pending && !error && page?.rows.length === 0 && (
        <div className="du-history-empty">
          <Text>
            {hasFilters
              ? text(
                  "ui.history.no_matches",
                  "No history matches these filters.",
                )
              : text("ui.history.empty", "No operations recorded yet.")}
          </Text>
        </div>
      )}
      {page && shown > 0 && (
        <div className="du-history-table" ref={table}>
          <DataTable
            key={
              page.revision.deleteGeneration + ":" + page.revision.maxRecordSeq
            }
            label={text("ui.history.operation_history", "Operation history")}
            columns={[
              text(
                "ui.history.operation_recorded_target",
                "Operation / recorded target",
              ),
              text("ui.history.status", "Status"),
              text("ui.history.finished", "Finished"),
              text("ui.history.details", "Details"),
            ]}
            columnWidths={[212, 106, 116, 72]}
            density="compact"
            totalRows={hasMore ? -1 : shown}
            rows={page.rows}
            renderCells={(row) => rowCells(row, onOpen, displayTime, text)}
          />
        </div>
      )}
      {(shown > 0 || hasMore) && (
        <footer className="du-history-footer">
          <LiveStatus>
            {text(
              "ui.history.records_shown",
              shown === 1 ? "{count} record shown" : "{count} records shown",
              { count: BigInt(shown) },
            )}
          </LiveStatus>
          {hasMore && (
            <Action
              variant="quiet"
              onPress={(event) => {
                pagingFocused.current = document.activeElement === event.target;
                more();
              }}
              isPending={pending}
            >
              {text("ui.history.load_more", "Load more history")}
            </Action>
          )}
        </footer>
      )}
    </div>
  );
}

function rowCells(
  row: HistoryListRow,
  onOpen: (jobRef: string, target: HTMLElement) => void,
  displayTime: ReturnType<typeof useHistoryTime>,
  text: ReturnType<typeof useLocaleText>,
) {
  const { summary, jobRef } = row;
  const name = kindLabel(summary?.kindId, text);
  const target =
    summary?.scopeSummary ?? text("ui.history.unavailable", "Unavailable");
  const time = displayTime(summary?.finishedAtMs);
  const state = summary?.state;
  const unavailable = row.missingReasons
    .map((reason) => missingLabel(reason, text))
    .join(" ");
  return [
    <div className="du-history-operation" key="operation">
      <div title={name}>
        <Text>{name}</Text>
      </div>
      <div title={target}>
        <Text kind="muted">{target}</Text>
      </div>
    </div>,
    <div
      key="status"
      title={unavailable || (state && stateLabel(state, false, text))}
    >
      <StatusBadge tone={stateTone(state)}>
        {state
          ? stateLabel(state, true, text)
          : text("ui.history.unavailable", "Unavailable")}
      </StatusBadge>
      {unavailable && (
        <Text kind="muted">{text("ui.history.incomplete", "Incomplete")}</Text>
      )}
    </div>,
    time,
    jobRef ? (
      <Action
        key="open"
        variant="quiet"
        aria-label={text(
          "ui.history.view_details_aria",
          "View details: {name}, {time}, {target}",
          { name, time, target },
        )}
        onPress={(event) => {
          if (event.target instanceof HTMLElement) onOpen(jobRef, event.target);
        }}
      >
        {text("ui.history.view", "View")}
      </Action>
    ) : (
      <Text kind="muted">{text("ui.history.unavailable", "Unavailable")}</Text>
    ),
  ];
}
