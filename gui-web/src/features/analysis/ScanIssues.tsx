import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  Action,
  DataTable,
  Heading,
  NameText,
  Notice,
  Progress,
  SelectField,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import {
  SCAN_ISSUE_FILTERS,
  scanIssueKindText,
  scanIssueReasonText,
  scanIssueStageText,
  scanIssuesProblemText,
  scanIssuesRecoveryText,
} from "../../ipc";
import type {
  AnalysisClient,
  ItemRef,
  ResultReadProblem,
  ScanFailureKind,
  ScanFailureRow,
  ScanIssueFilter,
} from "../../ipc";
import { useScanFailures } from "../../state/scan-failures";
import type {
  ScanIssueFolder,
  ScanIssueLaneRequest,
} from "./scan-issue-filter";
import { ScanIssueFolderChip } from "./ScanIssueFolderChip";
import { ScanIssueStorageChip } from "./ScanIssueStorageChip";
import type { StorageFilter } from "./ScanIssueStorageChip";
import "../../theme/scan-issues.css";

type IssueSource = {
  client: AnalysisClient;
  resultVersion: string;
  /**
   * Asks the result view to pin its latest result; never called automatically
   * unless a running scan has already replaced the page being read.
   */
  onReopenLatest: () => void;
  running?: boolean | undefined;
  laneRequest?: ScanIssueLaneRequest | undefined;
};

function selectedIssueKind(
  filter: ScanIssueFilter,
): ScanFailureKind | undefined {
  if (filter === "all" || filter === "unreadable_locations") return undefined;
  return filter;
}

/** Exposes issues for the displayed result without changing the analysis scope. */
export function ScanIssues({
  client,
  resultVersion,
  onReopenLatest,
  running = false,
  laneRequest,
}: IssueSource) {
  const text = useLocaleText();
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState<ScanIssueFilter>("all");
  const [storageFilter, setStorageFilter] = useState<StorageFilter>();
  const [folderFilter, setFolderFilter] = useState<ScanIssueFolder>();
  const [allStorageNotice, setAllStorageNotice] = useState(false);
  const [focusTitleToken, setFocusTitleToken] = useState(0);
  const [listReady, setListReady] = useState(false);
  const [countText, setCountText] = useState("");
  const section = useRef<HTMLElement>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const storageChip = useRef<HTMLButtonElement>(null);
  const seenLaneRequest = useRef(0);
  const focusedTitleToken = useRef(0);

  useEffect(() => {
    if (!laneRequest || laneRequest.requestId === seenLaneRequest.current)
      return;
    seenLaneRequest.current = laneRequest.requestId;
    setOpen(true);
    setFilter("unreadable_locations");
    setStorageFilter(
      laneRequest.rootIndexes
        ? {
            rootIndexes: laneRequest.rootIndexes,
            storagePath: laneRequest.storagePath,
            storageTail: laneRequest.storageTail,
            storageLabel: laneRequest.storageLabel,
            storagePaths: laneRequest.storagePaths,
          }
        : undefined,
    );
    setFolderFilter(laneRequest.folder);
    setAllStorageNotice(laneRequest.allStorages);
    setListReady(false);
    setCountText("");
    setFocusTitleToken((value) => value + 1);
  }, [laneRequest]);

  useLayoutEffect(() => {
    if (
      !open ||
      !listReady ||
      focusTitleToken === 0 ||
      focusedTitleToken.current === focusTitleToken
    )
      return;
    focusedTitleToken.current = focusTitleToken;
    section.current?.scrollIntoView?.({ block: "start" });
    title.current?.focus({ preventScroll: true });
  }, [focusTitleToken, listReady, open]);

  const reportListState = useCallback((ready: boolean, count: string) => {
    setListReady(ready);
    setCountText((current) => (current === count ? current : count));
  }, []);

  const clearStorage = () => {
    setFocusTitleToken((value) => value + 1);
    setListReady(false);
    setCountText("");
    setStorageFilter(undefined);
  };

  const clearFolder = () => {
    setFocusTitleToken((value) => value + 1);
    setListReady(false);
    setCountText("");
    setFolderFilter(undefined);
  };

  const changeProblemType = (value: ScanIssueFilter) => {
    setFilter(value);
    setListReady(false);
    setCountText("");
  };

  const rootIndexes = storageFilter?.rootIndexes;
  const kind = selectedIssueKind(filter);
  const failuresOnly = filter === "unreadable_locations";
  return (
    <section
      ref={section}
      className="du-results du-scan-issues"
      aria-label={text("ui.analysis.issues_region", "Scan issues")}
    >
      <Action onPress={() => setOpen((value) => !value)}>
        {open
          ? text("ui.analysis.issues_hide", "Hide scan issues")
          : text("ui.analysis.issues_view", "View scan issues")}
      </Action>
      {open && (
        <>
          <Heading
            level={3}
            ref={title}
            programmaticFocus
            aria-label={
              countText
                ? `${text("ui.analysis.issues_title", "Scan issues")}. ${countText}`
                : undefined
            }
          >
            {text("ui.analysis.issues_title", "Scan issues")}
          </Heading>
          {(storageFilter ||
            folderFilter ||
            (allStorageNotice && failuresOnly)) && (
            <div className="du-scan-issues__conditions">
              {storageFilter && (
                <ScanIssueStorageChip
                  filter={storageFilter}
                  chipRef={storageChip}
                  onClear={clearStorage}
                />
              )}
              {folderFilter && (
                <ScanIssueFolderChip
                  folder={folderFilter}
                  onClear={clearFolder}
                />
              )}
              {allStorageNotice && failuresOnly && (
                <Text kind="muted" className="du-scan-issues__notice">
                  {text(
                    "ui.analysis.issues_filter_all_storages",
                    "Showing unreadable locations from all storage because this one can't be filtered on its own",
                  )}
                </Text>
              )}
            </div>
          )}
          <Text kind="muted">
            {text(
              "ui.analysis.issues_explanation",
              "These entries explain where the scan could not collect or verify information.",
            )}
          </Text>
          <SelectField
            label={text("ui.analysis.issue_type", "Problem type")}
            value={filter}
            options={SCAN_ISSUE_FILTERS.map(({ value, key, fallback }) => ({
              value,
              label: text(key, fallback),
            }))}
            onChange={changeProblemType}
          />
          <IssuePages
            client={client}
            resultVersion={resultVersion}
            kind={kind}
            rootIndexes={rootIndexes}
            underItem={folderFilter?.item}
            failuresOnly={failuresOnly}
            refreshKey={laneRequest?.requestId ?? 0}
            running={running}
            onReopenLatest={onReopenLatest}
            onReadState={reportListState}
          />
        </>
      )}
    </section>
  );
}

function IssuePages({
  client,
  resultVersion,
  kind,
  rootIndexes,
  underItem,
  failuresOnly,
  refreshKey,
  running,
  onReopenLatest,
  onReadState,
}: IssueSource & {
  kind: ScanFailureKind | undefined;
  rootIndexes: number[] | undefined;
  underItem: ItemRef | undefined;
  failuresOnly: boolean;
  refreshKey: number;
  onReadState: (ready: boolean, count: string) => void;
}) {
  const text = useLocaleText();
  const list = useScanFailures(
    client,
    resultVersion,
    kind,
    rootIndexes,
    failuresOnly,
    refreshKey,
    underItem,
  );
  const { rows, totalCount, problem } = list;
  const autoFollowKey = `${resultVersion}:${refreshKey}:${kind ?? ""}:${rootIndexes?.join(",") ?? ""}:${underItem?.itemNo ?? ""}:${failuresOnly}`;
  const autoFollowed = useRef("");

  useEffect(() => {
    const ready =
      !list.pending &&
      list.version === resultVersion &&
      (totalCount !== undefined || problem !== undefined);
    const count =
      totalCount === undefined
        ? ""
        : text(
            "ui.analysis.issues_count",
            "Showing {shown} of {total} issues",
            { shown: BigInt(rows.length), total: BigInt(totalCount) },
          );
    onReadState(ready, count);
  }, [
    list.pending,
    list.version,
    onReadState,
    problem,
    resultVersion,
    rows.length,
    text,
    totalCount,
  ]);

  useEffect(() => {
    if (
      !running ||
      problem !== "stale_version" ||
      autoFollowed.current === autoFollowKey
    )
      return;
    autoFollowed.current = autoFollowKey;
    onReopenLatest();
  }, [autoFollowKey, onReopenLatest, problem, running]);

  return (
    <>
      {list.pending && (
        <Progress
          label={text("ui.analysis.issues_reading", "Reading scan issues")}
        />
      )}
      {problem && !(running && problem === "stale_version") && (
        <IssueProblem
          problem={problem}
          pending={list.pending}
          // Reopening changes the result version, which starts a new first-page
          // read; retrying here would ask again for the replaced version.
          onRecover={problem === "failed" ? list.retry : onReopenLatest}
        />
      )}
      {totalCount !== undefined && (
        <Text kind="muted">
          {text(
            "ui.analysis.issues_count",
            "Showing {shown} of {total} issues",
            { shown: BigInt(rows.length), total: BigInt(totalCount) },
          )}
        </Text>
      )}
      {totalCount === 0 && (
        <Text>
          {text(
            "ui.analysis.issues_none",
            "No scan issues were recorded for this filter.",
          )}
        </Text>
      )}
      {rows.length > 0 && <IssueTable rows={rows} totalCount={totalCount} />}
      {list.nextCursor && !problem && (
        <Action isDisabled={list.pending} onPress={list.readMore}>
          {text("ui.analysis.issues_load_more", "Load more issues")}
        </Action>
      )}
    </>
  );
}

function IssueProblem({
  problem,
  pending,
  onRecover,
}: {
  problem: ResultReadProblem;
  pending: boolean;
  onRecover: () => void;
}) {
  const text = useLocaleText();
  return (
    <>
      <Notice tone="error">{scanIssuesProblemText(problem, text)}</Notice>
      <Action isDisabled={pending} onPress={onRecover}>
        {scanIssuesRecoveryText(problem, text)}
      </Action>
    </>
  );
}

function issueCells(
  row: ScanFailureRow,
  text: LocaleText,
  timestamp: ReturnType<typeof useTimestampText>,
) {
  return [
    <Text kind="path" key="path">
      {row.displayPath !== undefined ? (
        <NameText path name={row.displayPath} />
      ) : (
        text("ui.analysis.issue_path_unavailable", "Path unavailable")
      )}
    </Text>,
    scanIssueKindText(row.kind, text),
    scanIssueStageText(row.stage, text),
    scanIssueReasonText(row.reasonKey, text),
    row.observedAt
      ? timestamp(Date.parse(row.observedAt), () => row.observedAt ?? "")
      : text("ui.analysis.issue_observed_unknown", "Not recorded"),
  ];
}

function IssueTable({
  rows,
  totalCount,
}: {
  rows: readonly ScanFailureRow[];
  totalCount: number | undefined;
}) {
  const text = useLocaleText();
  const timestamp = useTimestampText();
  return (
    <DataTable
      label={text("ui.analysis.issue_entries", "Scan issue entries")}
      columns={[
        text("ui.analysis.issue_path", "Path"),
        text("ui.analysis.issue_kind", "Issue"),
        text("ui.analysis.issue_stage", "Stage"),
        text("ui.analysis.issue_reason", "Reason"),
        text("ui.analysis.issue_observed", "Observed"),
      ]}
      columnWidths={[320, 180, 180, 240, 200]}
      density="compact"
      rows={rows}
      {...(totalCount !== undefined && { totalRows: totalCount })}
      getRowLabel={(_, index) =>
        text("ui.analysis.issue_number", "Issue {number}", {
          number: BigInt(index + 1),
        })
      }
      renderCells={(row) => issueCells(row, text, timestamp)}
    />
  );
}
