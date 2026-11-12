import type { SizeDisplay } from "../../i18n/bytes";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";
import {
  Action,
  Breadcrumbs,
  DetailDisclosure,
  Readout,
  canHandleBrowseKey,
  Heading,
  InspectionDialog,
  Notice,
  Progress,
  Text,
} from "../../components";
import type {
  BrowseOpenRequest,
  BrowseOpenResponse,
  BrowseCopyPathOutcome,
  FolderQueryRow,
  ItemRef,
  QueryPageRequest,
  QueryPageResponse,
} from "../../ipc";
import { BROWSE_PAGE_SIZE, BrowsePages } from "../../state/browse-pages";
import { folderPath, rowKey } from "./browse-display";
import { BrowseResults } from "./BrowseResults";
import { useItemInspection } from "./use-item-inspection";
import type { InspectedItem } from "./use-item-inspection";

/** Injected boundary for opening folders and paging through a pinned result. */
export interface BrowsePort {
  open(
    request: Omit<BrowseOpenRequest, "regionRef">,
    signal?: AbortSignal,
  ): Promise<BrowseOpenResponse>;
  page(request: QueryPageRequest): Promise<QueryPageResponse>;
  copyPath?(item: ItemRef): Promise<BrowseCopyPathOutcome>;
}

export type BrowseOpenLocation = Extract<
  BrowseOpenResponse,
  { kind: "opened" }
>;

type Opened = Extract<BrowseOpenResponse, { kind: "opened" }>;
type View = { opened: Opened; pages: BrowsePages };
type ParentLocation = { item?: ItemRef };
type InspectItem = (item: ItemRef, kind: InspectedItem["kind"]) => ReactNode;

function BrowseItemDialog({
  inspection,
  inspectItem,
  close,
}: {
  inspection: InspectedItem | undefined;
  inspectItem: InspectItem | undefined;
  close: () => void;
}) {
  if (!inspection || !inspectItem) return null;
  const title =
    inspection.kind === "folder" ? "Folder details" : "File details";
  return (
    <InspectionDialog title={title} onClose={close}>
      {inspectItem(inspection.item, inspection.kind)}
    </InspectionDialog>
  );
}

function parentLocation(
  opened: Opened,
  knownVirtualRoot: boolean,
): ParentLocation | undefined {
  const { rootKind, location, breadcrumbs, page } = opened;
  if (!location || location.resultVersion !== page.resultVersion)
    return undefined;
  if (rootKind === "scope_root") return knownVirtualRoot ? {} : undefined;
  if (rootKind !== "folder") return undefined;
  const current = breadcrumbs.at(-1)?.item;
  const parent = breadcrumbs.at(-2)?.item;
  if (!current || rowKey(current) !== rowKey(location)) return undefined;
  return parent?.resultVersion === page.resultVersion
    ? { item: parent }
    : undefined;
}

function isPreviousResult(displayed?: string, selected?: string): boolean {
  return (
    displayed !== undefined && selected !== undefined && displayed !== selected
  );
}

function readOnlyBreadcrumbs(
  entries: ReturnType<typeof folderPath>,
  readOnly: boolean,
) {
  if (!readOnly) return entries;
  return entries.map((entry) => ({
    id: entry.id,
    label: entry.label,
    ...(entry.current === undefined ? {} : { current: entry.current }),
  }));
}

function PreviousResultNotice({ previous }: { previous: boolean }) {
  if (!previous) return null;
  return (
    <Notice>
      Showing the previous result. Open the selected result to navigate.
    </Notice>
  );
}

/** Isolated folder browser; its host supplies the transport and pinned result. */
export function BrowsePanel({
  port,
  resultVersion,
  inspectItem,
  onOpening,
  onOpened,
  display = { system: "iec", unit: "auto" },
}: {
  port: BrowsePort;
  resultVersion?: string;
  inspectItem?: InspectItem;
  onOpening?: () => void;
  onOpened?: (opened: BrowseOpenLocation) => void;
  display?: SizeDisplay;
}) {
  const [view, setView] = useState<View>();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<"error" | "job">();
  const sequence = useRef(0);
  const opening = useRef<AbortController | undefined>(undefined);
  const lastAttempt = useRef<{
    location: ItemRef | undefined;
    pinned: string | undefined;
  }>(undefined);
  const virtualRootVersion = useRef<string>(undefined);
  const panel = useRef<HTMLElement>(null);
  const focusAfterUpdate = useRef<Element>(undefined);
  const inspection = useItemInspection(
    resultVersion ?? view?.opened.page.resultVersion,
    panel,
  );

  function navigate(
    location?: ItemRef,
    pinned = resultVersion,
    retrying = false,
  ) {
    if (retrying && opening.current) return;
    lastAttempt.current = { location, pinned };
    const requestSeq = ++sequence.current;
    const active = document.activeElement;
    const restoreFocus =
      active !== panel.current && panel.current?.contains(active);

    opening.current?.abort();
    onOpening?.();
    const controller = new AbortController();
    opening.current = controller;
    setBusy(true);
    if (!retrying) setMessage(undefined);
    void port
      .open(
        {
          viewKind: "list",
          requestSeq,
          ...(pinned ? { resultVersion: pinned } : {}),
          ...(location ? { location } : {}),
          pageSize: BROWSE_PAGE_SIZE,
        },
        controller.signal,
      )
      .then((response) => {
        if (sequence.current !== requestSeq) return;
        opening.current = undefined;
        setBusy(false);
        if (
          response.requestSeq !== requestSeq ||
          (pinned &&
            (response.kind === "job"
              ? response.resultVersion !== pinned
              : response.page.resultVersion !== pinned))
        ) {
          setMessage("error");
          return;
        }
        if (response.kind === "job") {
          setMessage("job");
          return;
        }
        const pages = new BrowsePages(response.page);
        onOpened?.(response);
        if (response.rootKind === "virtual_root") {
          virtualRootVersion.current = response.page.resultVersion;
        }
        focusAfterUpdate.current =
          restoreFocus && document.activeElement === active
            ? (active ?? undefined)
            : undefined;
        setMessage(undefined);
        setView({
          opened: { ...response, page: { ...response.page, rows: [] } },
          pages,
        });
      })
      .catch(() => {
        if (sequence.current !== requestSeq) return;
        opening.current = undefined;
        setBusy(false);
        setMessage("error");
      });
  }

  useEffect(() => {
    inspection.reset();
    virtualRootVersion.current = undefined;
    navigate(undefined, resultVersion);
    return () => {
      sequence.current += 1;
      opening.current?.abort();
    };
    // The injected port and requested result identify a new browser session.
  }, [port, resultVersion]);

  useLayoutEffect(() => {
    const focus = focusAfterUpdate.current;
    focusAfterUpdate.current = undefined;
    if (
      focus &&
      (document.activeElement === focus ||
        (!focus.isConnected && document.activeElement === document.body))
    )
      panel.current?.focus();
  }, [view]);

  const page = view?.opened.page;
  const previousResult = isPreviousResult(page?.resultVersion, resultVersion);
  const hasVirtualRoot =
    page !== undefined && virtualRootVersion.current === page.resultVersion;
  const parent = view && parentLocation(view.opened, hasVirtualRoot);

  function openItem(row: FolderQueryRow, trigger: Element | null) {
    if (!opening.current) inspection.open(row, trigger);
  }

  function goUp() {
    if (parent && page && !opening.current && !previousResult)
      navigate(parent.item, page.resultVersion);
  }

  function handleBrowseKey(event: KeyboardEvent<HTMLElement>) {
    if (
      event.key !== "ArrowUp" ||
      !event.altKey ||
      !parent ||
      !canHandleBrowseKey(event)
    )
      return;
    event.preventDefault();
    if (!event.repeat) goUp();
  }

  return (
    <section
      ref={panel}
      tabIndex={-1}
      className="du-results du-browse-panel"
      aria-label="Folder navigation"
      onKeyDownCapture={handleBrowseKey}
    >
      <div className="du-browse-header">
        <Heading level={2}>Folders and files</Heading>
        {busy && <Progress label="Opening folder" />}
        {message === "error" && (
          <Notice tone="error">
            Could not open this folder.{" "}
            <Action
              isPending={busy}
              onPress={() =>
                navigate(
                  lastAttempt.current?.location,
                  lastAttempt.current?.pinned,
                  true,
                )
              }
            >
              {busy ? "Retrying…" : "Retry"}
            </Action>
          </Notice>
        )}
        {message === "job" && (
          <Notice>Folder results are being prepared.</Notice>
        )}
      </div>
      {view && page && (
        <>
          <div className="du-browse-header">
            <PreviousResultNotice previous={previousResult} />
            <div className="du-browse-context">
              <div className="du-card__actions">
                <Action onPress={goUp} isDisabled={!parent || previousResult}>
                  Up
                </Action>
                <Breadcrumbs
                  label="Folder path"
                  entries={readOnlyBreadcrumbs(
                    folderPath(view.opened, hasVirtualRoot, (item) =>
                      navigate(item, page.resultVersion),
                    ),
                    previousResult,
                  )}
                />
              </div>
            </div>
            <ObservationSummary
              opened={view.opened}
              hasVirtualRoot={hasVirtualRoot}
            />
            {page.completeness !== "complete" && (
              <Notice>Results are incomplete.</Notice>
            )}
          </div>
          <BrowseResults
            key={view.opened.requestSeq}
            pages={view.pages}
            display={display}
            initialCursor={page.nextCursor}
            readPage={(request) => port.page(request)}
            busy={busy || previousResult}
            readOnly={previousResult}
            navigationEpoch={sequence.current}
            navigationSeq={() => sequence.current}
            canRead={() => opening.current === undefined && !previousResult}
            focusPanel={() => panel.current?.focus({ preventScroll: true })}
            openFolder={(item) => navigate(item, page.resultVersion)}
            openItem={inspectItem ? openItem : undefined}
            copyPath={port.copyPath}
            reopen={() =>
              navigate(
                view.opened.location ?? undefined,
                page.resultVersion,
                true,
              )
            }
          />
        </>
      )}
      <BrowseItemDialog
        inspection={inspection.selected}
        inspectItem={inspectItem}
        close={inspection.close}
      />
    </section>
  );
}

function ObservationSummary({
  opened,
  hasVirtualRoot,
}: {
  opened: Opened;
  hasVirtualRoot: boolean;
}) {
  const page = opened.page;
  return (
    <div className="du-browse-observation">
      <Text kind="muted">
        {opened.origin} · {page.observedAt?.slice(0, 10) ?? "Unavailable"} ·{" "}
        {page.total.state === "lower_bound" ? "At least " : ""}
        {page.total.count} items
        {page.completeness === "complete" ? " · Complete" : ""}
      </Text>
      <DetailDisclosure title="Observation details">
        <section tabIndex={0} aria-label="Full folder location and observation">
          <Readout
            label="Folder location and observation"
            rows={[
              {
                label: "Location hierarchy",
                value: folderPath(opened, hasVirtualRoot, () => undefined)
                  .map((entry) => entry.label)
                  .join(" / "),
                wideValue: true,
              },
              { label: "Source", value: opened.origin },
              {
                label: "Observed",
                value: page.observedAt ?? "Unavailable",
                wideValue: true,
              },
              { label: "Result", value: "Pinned result" },
              {
                label: "Completeness",
                value:
                  page.completeness === "complete" ? "Complete" : "Incomplete",
              },
            ]}
          />
          {page.unknownCount > 0 && (
            <Text kind="muted">
              {page.unknownCount} items could not be evaluated.
            </Text>
          )}
          {page.completeness !== "complete" && (
            <Text>Some items may be unavailable.</Text>
          )}
        </section>
      </DetailDisclosure>
    </div>
  );
}
