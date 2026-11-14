import { useState } from "react";
import {
  Action,
  DataTable,
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
  ResultReadProblem,
  ScanFailureKind,
  ScanFailureRow,
  ScanIssueFilter,
} from "../../ipc";
import { useScanFailures } from "../../state/scan-failures";

type IssueSource = {
  client: AnalysisClient;
  resultVersion: string;
  /** Asks the result view to pin its latest result; never called automatically. */
  onReopenLatest: () => void;
};

/** Exposes issues for the displayed result without changing the analysis scope. */
export function ScanIssues({
  client,
  resultVersion,
  onReopenLatest,
}: IssueSource) {
  const text = useLocaleText();
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState<ScanIssueFilter>("all");
  return (
    <section
      className="du-results"
      aria-label={text("ui.analysis.issues_region", "Scan issues")}
    >
      <Action onPress={() => setOpen((value) => !value)}>
        {open
          ? text("ui.analysis.issues_hide", "Hide scan issues")
          : text("ui.analysis.issues_view", "View scan issues")}
      </Action>
      {open && (
        <>
          <Text kind="muted">
            {text(
              "ui.analysis.issues_explanation",
              "These entries explain where the scan could not collect or verify information.",
            )}
          </Text>
          <SelectField
            label={text("ui.analysis.issue_type", "Issue type")}
            value={filter}
            options={SCAN_ISSUE_FILTERS.map(({ value, key, fallback }) => ({
              value,
              label: text(key, fallback),
            }))}
            onChange={setFilter}
          />
          <IssuePages
            client={client}
            resultVersion={resultVersion}
            kind={filter === "all" ? undefined : filter}
            onReopenLatest={onReopenLatest}
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
  onReopenLatest,
}: IssueSource & { kind: ScanFailureKind | undefined }) {
  const text = useLocaleText();
  const list = useScanFailures(client, resultVersion, kind);
  const { rows, totalCount, problem } = list;
  return (
    <>
      {list.pending && (
        <Progress
          label={text("ui.analysis.issues_reading", "Reading scan issues")}
        />
      )}
      {problem && (
        <IssueProblem
          problem={problem}
          pending={list.pending}
          onRecover={
            problem === "failed"
              ? list.retry
              : () => {
                  onReopenLatest();
                  list.retry();
                }
          }
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
      {row.displayPath ??
        text("ui.analysis.issue_path_unavailable", "Path unavailable")}
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
