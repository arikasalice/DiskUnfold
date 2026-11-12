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
    <section className="du-history-shell" aria-label="Operation history">
      <div ref={list} className="du-history-screen" hidden={Boolean(selected)}>
        <header className="du-history-header">
          <Heading level={2}>Operation history</Heading>
          <Action
            variant="quiet"
            icon="back"
            ref={back}
            autoFocus
            onPress={onClose}
          >
            Back to analysis
          </Action>
        </header>
        <div className="du-history-controls">
          <div className="du-history-filter-row">
            <SelectField
              label="End state"
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
                Clear filters
              </Action>
            )}
          </div>
          <DetailDisclosure
            title={
              extraFilters
                ? "Date and type filters (active)"
                : "Date and type filters"
            }
          >
            <div className="du-history-extra-filters">
              <DateField label="From date" value={from} onChange={setFrom} />
              <DateField
                label="Through date"
                value={through}
                onChange={setThrough}
              />
              <SelectField
                label="Job type"
                value={kind}
                options={kinds}
                onChange={setKind}
              />
            </div>
          </DetailDisclosure>
        </div>
        {invalid ? (
          <Notice tone="error">Choose a valid date range.</Notice>
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
        <Notice>History changed. Showing the first page again.</Notice>
      )}
      {pending && <Progress label="Loading history" />}
      {error && (
        <Notice tone="error">
          History could not be loaded. <Action onPress={retry}>Retry</Action>
        </Notice>
      )}
      {!pending && !error && page?.rows.length === 0 && (
        <div className="du-history-empty">
          <Text>
            {hasFilters
              ? "No history matches these filters."
              : "No operations recorded yet."}
          </Text>
        </div>
      )}
      {page && shown > 0 && (
        <div className="du-history-table" ref={table}>
          <DataTable
            key={
              page.revision.deleteGeneration + ":" + page.revision.maxRecordSeq
            }
            label="Operation history"
            columns={[
              "Operation / recorded target",
              "Status",
              "Finished",
              "Details",
            ]}
            columnWidths={[212, 106, 116, 72]}
            density="compact"
            totalRows={hasMore ? -1 : shown}
            rows={page.rows}
            renderCells={(row) => rowCells(row, onOpen, displayTime)}
          />
        </div>
      )}
      {(shown > 0 || hasMore) && (
        <footer className="du-history-footer">
          <LiveStatus>
            {shown.toLocaleString()} {shown === 1 ? "record" : "records"} shown
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
              Load more history
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
) {
  const { summary, jobRef } = row;
  const name = kindLabel(summary?.kindId);
  const target = summary?.scopeSummary ?? "Unavailable";
  const time = displayTime(summary?.finishedAtMs);
  const state = summary?.state;
  const unavailable = row.missingReasons.map(missingLabel).join(" ");
  return [
    <div className="du-history-operation" key="operation">
      <div title={name}>
        <Text>{name}</Text>
      </div>
      <div title={target}>
        <Text kind="muted">{target}</Text>
      </div>
    </div>,
    <div key="status" title={unavailable || (state && stateLabel(state))}>
      <StatusBadge tone={stateTone(state)}>
        {state ? stateLabel(state, true) : "Unavailable"}
      </StatusBadge>
      {unavailable && <Text kind="muted">Incomplete</Text>}
    </div>,
    time,
    jobRef ? (
      <Action
        key="open"
        variant="quiet"
        aria-label={"View details: " + name + ", " + time + ", " + target}
        onPress={(event) => {
          if (event.target instanceof HTMLElement) onOpen(jobRef, event.target);
        }}
      >
        View
      </Action>
    ) : (
      <Text kind="muted">Unavailable</Text>
    ),
  ];
}
