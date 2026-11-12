import { useState } from "react";
import {
  Action,
  Notice,
  Progress,
  Readout,
  SelectField,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { AnalysisClient, ScanFailureKind } from "../../ipc";
import { useScanFailures } from "../../state/scan-failures";

const filters = [
  { value: "all", key: "ui.analysis.issues_all", fallback: "All issues" },
  {
    value: "collection_failed",
    key: "ui.analysis.issues_collection_failures",
    fallback: "Collection failures",
  },
  {
    value: "unread_range",
    key: "ui.analysis.issues_unread_ranges",
    fallback: "Unread ranges",
  },
  {
    value: "uncertain",
    key: "ui.analysis.issues_uncertain_entries",
    fallback: "Uncertain entries",
  },
  {
    value: "untraversed_link",
    key: "ui.analysis.issues_untraversed_links",
    fallback: "Untraversed links",
  },
] as const;
const kinds = new Map<string, readonly [key: string, fallback: string]>(
  filters.map(({ value, key, fallback }) => [value, [key, fallback]]),
);
const stages = new Map<string, readonly [key: string, fallback: string]>([
  ["metadata", ["ui.analysis.issue_stage_metadata", "Metadata"]],
  [
    "open_directory",
    ["ui.analysis.issue_stage_open_directory", "Opening a folder"],
  ],
  ["enumerate", ["ui.analysis.issue_stage_enumerate", "Listing a folder"]],
  ["traverse", ["ui.analysis.issue_stage_traverse", "Link traversal"]],
]);
const reasons = new Map<string, readonly [key: string, fallback: string]>([
  [
    "scan.path_denied",
    ["ui.analysis.issue_reason_access_denied", "Access denied"],
  ],
  [
    "scan.path_vanished",
    [
      "ui.analysis.issue_reason_path_missing",
      "The item was no longer available",
    ],
  ],
  [
    "scan.unclassified",
    ["ui.analysis.issue_reason_unverified", "The item could not be verified"],
  ],
  [
    "scan.link_untraversed",
    ["ui.analysis.issue_reason_link_skipped", "The link target was skipped"],
  ],
]);

type IssueFilter = (typeof filters)[number]["value"];
type IssueSource = { client: AnalysisClient; resultVersion: string };

function mappedLabel(
  entries: ReadonlyMap<string, readonly [key: string, fallback: string]>,
  code: string,
  unknownKey: string,
  unknownFallback: string,
  text: ReturnType<typeof useLocaleText>,
) {
  const message = entries.get(code);
  return message
    ? text(message[0], message[1])
    : text(unknownKey, unknownFallback);
}

/** Exposes issues for the displayed result without changing the analysis scope. */
export function ScanIssues({ client, resultVersion }: IssueSource) {
  const text = useLocaleText();
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState<IssueFilter>("all");
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
            options={filters.map(({ value, key, fallback }) => ({
              value,
              label: text(key, fallback),
            }))}
            onChange={setFilter}
          />
          <IssuePages
            key={filter}
            client={client}
            resultVersion={resultVersion}
            kind={filter === "all" ? undefined : filter}
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
}: IssueSource & { kind: ScanFailureKind | undefined }) {
  const text = useLocaleText();
  const { page, pending, error, readMore } = useScanFailures(
    client,
    resultVersion,
    kind,
  );
  return (
    <>
      {pending && (
        <Progress
          label={text("ui.analysis.issues_reading", "Reading scan issues")}
        />
      )}
      {error && <Notice tone="error">{error}</Notice>}
      {page && (
        <Text kind="muted">
          {text(
            "ui.analysis.issues_count",
            "Showing {shown} of {total} issues",
            { shown: BigInt(page.rows.length), total: BigInt(page.totalCount) },
          )}
        </Text>
      )}
      {page?.totalCount === 0 && (
        <Text>
          {text(
            "ui.analysis.issues_none",
            "No scan issues were recorded for this filter.",
          )}
        </Text>
      )}
      {page && (
        <Readout
          label={text("ui.analysis.issue_entries", "Scan issue entries")}
          rows={page.rows.map((row, index) => ({
            label: text("ui.analysis.issue_number", "Issue {number}", {
              number: BigInt(index + 1),
            }),
            value: (
              <>
                <Text kind="path">
                  {row.displayPath ??
                    text(
                      "ui.analysis.issue_path_unavailable",
                      "Path unavailable",
                    )}
                </Text>
                <Text>
                  {mappedLabel(
                    kinds,
                    row.kind,
                    "ui.analysis.issue_unknown_kind",
                    "Unknown issue",
                    text,
                  )}
                  {" · "}
                  {mappedLabel(
                    stages,
                    row.stage,
                    "ui.analysis.issue_unknown_stage",
                    "Unknown stage",
                    text,
                  )}
                  {" · "}
                  {mappedLabel(
                    reasons,
                    row.reasonKey,
                    "ui.analysis.issue_unknown_reason",
                    "Unknown reason",
                    text,
                  )}
                </Text>
                {row.observedAt && <Text kind="muted">{row.observedAt}</Text>}
              </>
            ),
          }))}
        />
      )}
      {(page?.nextCursor || error) && (
        <Action isDisabled={pending} onPress={readMore}>
          {error
            ? text("ui.analysis.issues_retry", "Retry scan issues")
            : text("ui.analysis.issues_load_more", "Load more issues")}
        </Action>
      )}
    </>
  );
}
