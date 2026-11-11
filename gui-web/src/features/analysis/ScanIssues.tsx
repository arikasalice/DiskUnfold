import { useState } from "react";
import {
  Action,
  Notice,
  Progress,
  Readout,
  SelectField,
  Text,
} from "../../components";
import type { AnalysisClient, ScanFailureKind } from "../../ipc";
import { useScanFailures } from "../../state/scan-failures";

const filters = [
  { value: "all", label: "All issues" },
  { value: "collection_failed", label: "Collection failures" },
  { value: "unread_range", label: "Unread ranges" },
  { value: "uncertain", label: "Uncertain entries" },
  { value: "untraversed_link", label: "Untraversed links" },
] as const;
const stages = new Map([
  ["metadata", "Metadata"],
  ["open_directory", "Opening a folder"],
  ["enumerate", "Listing a folder"],
  ["traverse", "Link traversal"],
]);
const reasons = new Map([
  ["scan.path_denied", "Access denied"],
  ["scan.path_vanished", "The item was no longer available"],
  ["scan.unclassified", "The item could not be verified"],
  ["scan.link_untraversed", "The link target was skipped"],
]);

type IssueFilter = (typeof filters)[number]["value"];
type IssueSource = { client: AnalysisClient; resultVersion: string };

/** Exposes issues for the displayed result without changing the analysis scope. */
export function ScanIssues({ client, resultVersion }: IssueSource) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState<IssueFilter>("all");
  return (
    <section className="du-results" aria-label="Scan issues">
      <Action onPress={() => setOpen((value) => !value)}>
        {open ? "Hide scan issues" : "View scan issues"}
      </Action>
      {open && (
        <>
          <Text kind="muted">
            These entries explain where the scan could not collect or verify
            information.
          </Text>
          <SelectField
            label="Issue type"
            value={filter}
            options={filters}
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
  const { page, pending, error, readMore } = useScanFailures(
    client,
    resultVersion,
    kind,
  );
  return (
    <>
      {pending && <Progress label="Reading scan issues" />}
      {error && <Notice tone="error">{error}</Notice>}
      {page && (
        <Text kind="muted">
          Showing {page.rows.length} of {page.totalCount} issues
        </Text>
      )}
      {page?.totalCount === 0 && (
        <Text>No scan issues were recorded for this filter.</Text>
      )}
      {page && (
        <Readout
          label="Scan issue entries"
          rows={page.rows.map((row, index) => ({
            label: `Issue ${index + 1}`,
            value: (
              <>
                <Text kind="path">{row.displayPath ?? "Path unavailable"}</Text>
                <Text>
                  {filters.find(({ value }) => value === row.kind)?.label ??
                    "Unknown issue"}
                  {" · "}
                  {stages.get(row.stage) ?? "Unknown stage"}
                  {" · "}
                  {reasons.get(row.reasonKey) ?? "Unknown reason"}
                </Text>
                {row.observedAt && <Text kind="muted">{row.observedAt}</Text>}
              </>
            ),
          }))}
        />
      )}
      {(page?.nextCursor || error) && (
        <Action isDisabled={pending} onPress={readMore}>
          {error ? "Retry scan issues" : "Load more issues"}
        </Action>
      )}
    </>
  );
}
