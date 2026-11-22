import type { SizeDisplay } from "../../i18n/bytes";
import { defaultSizeDisplay } from "../../state/size-display";
import { useLocaleText } from "../../i18n/presentation";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";
import {
  Action,
  Breadcrumbs,
  BROWSE_SCREEN,
  browseKeyTable,
  dispatchKey,
  Heading,
  Notice,
  Progress,
} from "../../components";
import type {
  BrowseOpenRequest,
  BrowseOpenResponse,
  BrowseCopyPathOutcome,
  FolderQueryRow,
  ItemRef,
  QueryPageRequest,
  QueryPageResponse,
  SpaceTotal,
} from "../../ipc";
import { BROWSE_PAGE_SIZE, BrowsePages } from "../../state/browse-pages";
import type { ScanPriority } from "../../state/scan-priority";
import { useLocaleOsFamily } from "../../state/use-locale";
import { folderPath } from "./browse-display";
import { BrowseResults } from "./BrowseResults";
import type { ListEntryFocus, ListRowFocus } from "./BrowseResults";
import { useBrowseDedupLimit } from "./use-browse-dedup-limit";
import type { InspectedItem } from "./use-item-inspection";
import {
  BrowseItemDialog,
  ObservationSummary,
  PreviousResultNotice,
  isPreviousResult,
  parentLocation,
  readOnlyBreadcrumbs,
} from "./browse-panel-parts";
import type { InspectItem } from "./browse-panel-parts";
import { useItemInspection } from "./use-item-inspection";
import {
  priorView,
  PreviousFolderAction,
  restorePrevious,
} from "./browse-history";
import type { View } from "./browse-history";

/** Injected boundary for opening folders and paging through a pinned result. */
export interface BrowsePort {
  open(
    request: Omit<BrowseOpenRequest, "regionRef">,
    signal?: AbortSignal,
  ): Promise<BrowseOpenResponse>;
  page(request: QueryPageRequest): Promise<QueryPageResponse>;
  spaceTotal?(resultVersion: string): Promise<SpaceTotal>;
  copyPath?(item: ItemRef): Promise<BrowseCopyPathOutcome>;
}

export type BrowseOpenLocation = Extract<
  BrowseOpenResponse,
  { kind: "opened" }
>;

/**
 * The control that held focus when a folder was opened; a pressed control
 * that is still usable afterwards keeps the focus.
 */
type FocusOrigin = { origin: Element; pressed: boolean };

/** Isolated folder browser; its host supplies the transport and pinned result. */
export function BrowsePanel({
  port,
  resultVersion,
  inspectItem,
  onOpening,
  onOpened,
  display,
  priority,
  rootLabel,
  viewUnread,
  actionsDisabledReason,
  titleBadge,
  focusTitle = false,
}: {
  port: BrowsePort;
  resultVersion?: string;
  inspectItem?: InspectItem;
  onOpening?: () => void;
  onOpened?: (opened: BrowseOpenLocation) => void;
  display?: SizeDisplay;
  /** "Analyze first" for a running scan's current partial result, when available. */
  priority?: ScanPriority;
  /** Opens the unreadable locations below a folder in the analysis view (F02 #358). */
  viewUnread?: (item: ItemRef, name: string) => void;
  /** Names the top of the folder path instead of "Scope roots", e.g. for the sample root. */
  rootLabel?: string;
  /**
   * Turns off the actions that act on a real path, such as copying it, and
   * shows this reason beside them. The sample experience sets it because its
   * items are not files on the disk.
   */
  actionsDisabledReason?: string;
  /** A badge shown beside the title, such as the sample experience's "Example" mark. */
  titleBadge?: ReactNode;
  /**
   * Moves focus to the title when the panel mounts, for a host that shows the
   * panel as its own screen. The title draws no focus frame; it is also the
   * last fallback for focus that has no row or "Up" to go to.
   */
  focusTitle?: boolean;
}) {
  const osFamily = useLocaleOsFamily();
  const effectiveDisplay = display ?? defaultSizeDisplay(osFamily);
  const text = useLocaleText();
  const [view, setView] = useState<View>();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<"error" | "job">();
  // Counts openings that ended without a new list, so a selection action bar
  // waiting for "Show latest result" learns that the switch failed.
  const [openFailures, setOpenFailures] = useState(0);
  const sequence = useRef(0);
  const opening = useRef<AbortController | undefined>(undefined);
  const lastAttempt = useRef<{
    location: ItemRef | undefined;
    pinned: string | undefined;
  }>(undefined);
  const virtualRootVersion = useRef<string>(undefined);
  const panel = useRef<HTMLElement>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const upAction = useRef<HTMLButtonElement>(null);
  const listEntry = useRef<ListEntryFocus>(undefined);
  const listRow = useRef<ListRowFocus>(undefined);
  const focusAfterUpdate = useRef<FocusOrigin>(undefined);
  // Survives the remount of the results, so a new list's selection action
  // bar can take over the focus its predecessor held.
  const barFocus = useRef(false);
  const inspection = useItemInspection(
    resultVersion ?? view?.opened.page.resultVersion,
    (index) => {
      if (listRow.current) listRow.current(index);
      else if (!focusUp()) focusTitleNow();
    },
  );

  function navigate(
    location?: ItemRef,
    pinned = resultVersion,
    retrying = false,
    pressed = false,
    rememberCurrent = false,
    inspectionToRestore?: InspectedItem,
  ) {
    if (retrying && opening.current) return;
    lastAttempt.current = { location, pinned };
    const requestSeq = ++sequence.current;
    const active = document.activeElement;
    // The title holds the screen's entry focus; opening the first list keeps it there.
    const restoreFocus =
      active !== panel.current &&
      active !== title.current &&
      panel.current?.contains(active);

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
          setOpenFailures((count) => count + 1);
          return;
        }
        if (response.kind === "job") {
          setMessage("job");
          setOpenFailures((count) => count + 1);
          return;
        }
        const pages = new BrowsePages(response.page);
        onOpened?.(response);
        if (response.rootKind === "virtual_root") {
          virtualRootVersion.current = response.page.resultVersion;
        }
        focusAfterUpdate.current =
          restoreFocus && active && document.activeElement === active
            ? { origin: active, pressed }
            : undefined;
        setMessage(undefined);
        const previous = priorView(
          view,
          response.page.resultVersion,
          location,
          retrying,
          inspectionToRestore ?? inspection.selected,
          rememberCurrent,
        );
        setView({
          opened: { ...response, page: { ...response.page, rows: [] } },
          pages,
          ...(previous ? { previous } : {}),
        });
      })
      .catch(() => {
        if (sequence.current !== requestSeq) return;
        opening.current = undefined;
        setBusy(false);
        setMessage("error");
        setOpenFailures((count) => count + 1);
      });
  }

  function retry() {
    navigate(lastAttempt.current?.location, lastAttempt.current?.pinned, true);
  }

  /** Focuses "Up" when it is available; false when it is not. */
  function focusUp(): boolean {
    const up = upAction.current;
    if (!up || up.disabled) return false;
    up.focus({ preventScroll: true });
    return document.activeElement === up;
  }

  function focusTitleNow() {
    title.current?.focus({ preventScroll: true });
  }

  useEffect(() => {
    if (focusTitle) title.current?.focus({ preventScroll: true });
    // Only the screen's entry moves focus to the title.
  }, []);

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
    const pending = focusAfterUpdate.current;
    focusAfterUpdate.current = undefined;
    if (!pending) return;
    const { origin, pressed } = pending;
    const active = document.activeElement;
    // A pressed button that is still usable keeps the focus, so "Up" can be
    // pressed again.
    if (pressed && active === origin && !origin.matches(":disabled")) return;
    // Focus was lost with the control that opened the folder when that control
    // went away, or became disabled ("Up" at the top), which the browser
    // answers by moving focus to the body.
    if (
      active === origin ||
      (active === document.body &&
        (!origin.isConnected || origin.matches(":disabled")))
    )
      listEntry.current?.(origin);
  }, [view]);

  const page = view?.opened.page;
  const dedupCapacityLowerBound = useBrowseDedupLimit(
    port.spaceTotal,
    page?.resultVersion,
  );
  const previousResult = isPreviousResult(page?.resultVersion, resultVersion);
  const hasVirtualRoot =
    page !== undefined && virtualRootVersion.current === page.resultVersion;
  const parent = view && parentLocation(view.opened, hasVirtualRoot);

  function openItem(row: FolderQueryRow, trigger: Element | null) {
    if (!opening.current) inspection.open(row, trigger);
  }

  function goUp(pressed = false) {
    if (parent && page && !opening.current && !previousResult)
      navigate(parent.item, page.resultVersion, false, pressed);
  }

  function handleBrowseKey(event: KeyboardEvent<HTMLElement>) {
    // The key assignment table row `go_up` (Alt+ArrowUp) decides the action;
    // dispatchKey applies composition, input and dialog precedence first.
    if (
      !parent ||
      !(event.target instanceof Node) ||
      !event.currentTarget.contains(event.target)
    )
      return;
    const dispatched = dispatchKey(
      event.nativeEvent,
      browseKeyTable,
      BROWSE_SCREEN,
    );
    if (dispatched.stage !== "table" || dispatched.action !== "go_up") return;
    event.preventDefault();
    if (!event.repeat) goUp();
  }

  return (
    <section
      ref={panel}
      tabIndex={-1}
      className="du-results du-browse-panel"
      aria-label={text("ui.browse.folder_navigation", "Folder navigation")}
      onKeyDownCapture={handleBrowseKey}
    >
      <div className="du-browse-header">
        <div className="du-browse-title">
          <Heading level={2} ref={title} programmaticFocus>
            {text("ui.browse.folders_and_files", "Folders and files")}
          </Heading>
          {titleBadge}
        </div>
        {busy && (
          <Progress
            label={text("ui.browse.opening_folder", "Opening folder")}
          />
        )}
        {message === "error" && (
          <Notice tone="error">
            {text(
              "ui.browse.open_folder_failed",
              "Could not open this folder.",
            )}{" "}
            <Action isPending={busy} onPress={retry}>
              {busy
                ? text("ui.browse.retrying", "Retrying…")
                : text("ui.browse.retry", "Retry")}
            </Action>
          </Notice>
        )}
        {message === "job" && (
          <Notice>
            {text(
              "ui.browse.preparing_results",
              "Folder results are being prepared.",
            )}
          </Notice>
        )}
      </div>
      {view && page && (
        <>
          <div className="du-browse-header">
            <PreviousResultNotice previous={previousResult} />
            <div className="du-browse-context">
              <PreviousFolderAction
                previous={view.previous}
                text={text}
                onPress={() =>
                  restorePrevious(
                    view,
                    setView,
                    inspection.restore,
                    inspection.reset,
                    busy,
                  )
                }
              />
              <div className="du-card__actions">
                <Action
                  ref={upAction}
                  onPress={() => goUp(true)}
                  isDisabled={!parent || previousResult}
                >
                  {text("ui.browse.up", "Up")}
                </Action>
                <Breadcrumbs
                  label={text("ui.browse.folder_path", "Folder path")}
                  entries={readOnlyBreadcrumbs(
                    folderPath(
                      view.opened,
                      hasVirtualRoot,
                      (item) => navigate(item, page.resultVersion),
                      text,
                      rootLabel,
                    ),
                    previousResult,
                  )}
                />
              </div>
            </div>
            <ObservationSummary
              opened={view.opened}
              hasVirtualRoot={hasVirtualRoot}
              rootLabel={rootLabel}
            />
          </div>
          <BrowseResults
            key={view.opened.requestSeq}
            pages={view.pages}
            incomplete={page.completeness !== "complete"}
            dedupCapacityLowerBound={dedupCapacityLowerBound}
            display={effectiveDisplay}
            initialCursor={page.nextCursor}
            readPage={(request) => port.page(request)}
            busy={busy || previousResult}
            readOnly={previousResult}
            navigationEpoch={sequence.current}
            navigationSeq={() => sequence.current}
            canRead={() => opening.current === undefined && !previousResult}
            focusTitle={focusTitleNow}
            focusUp={focusUp}
            listEntry={listEntry}
            listRow={listRow}
            barFocus={barFocus}
            openFailures={openFailures}
            retryOpen={retry}
            openFolder={(item) => navigate(item, page.resultVersion)}
            openItem={inspectItem ? openItem : undefined}
            copyPath={
              actionsDisabledReason === undefined ? port.copyPath : undefined
            }
            copyDisabledReason={actionsDisabledReason}
            priority={priority}
            viewUnread={viewUnread}
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
        navigateItem={(item) => {
          if (page && item.resultVersion === page.resultVersion) {
            const inspectionToRestore = inspection.selected;
            inspection.close();
            navigate(
              item,
              page.resultVersion,
              false,
              false,
              true,
              inspectionToRestore,
            );
          }
        }}
      />
    </section>
  );
}
