import {
  Action,
  Heading,
  Notice,
  Progress,
  Readout,
  Text,
} from "../../components";
import type { AnalysisClient, HistoryDetail } from "../../ipc";
import { useHistoryDetail } from "../../state/history-detail";
import { missingLabel, relationLabel, stateLabel } from "./history-labels";

/** Read-only operation detail; approval, restore and deletion remain separate. */
export function HistoryDetailPanel({
  client,
  jobRef,
  onNavigate,
  onClose,
}: {
  client: AnalysisClient;
  jobRef: string;
  onNavigate: (jobRef: string) => void;
  onClose: () => void;
}) {
  const { detail, pending, error, retry } = useHistoryDetail(client, jobRef);
  return (
    <section className="du-results" aria-labelledby="history-detail-heading">
      <div className="du-card__header">
        <Heading level={2} id="history-detail-heading">
          Operation details
        </Heading>
        <Action autoFocus onPress={onClose}>
          Back to history
        </Action>
      </div>
      {pending && <Progress label="Loading operation details" />}
      {error === "removed" && (
        <Notice>History was removed and can no longer be viewed.</Notice>
      )}
      {error === "storage" && (
        <Notice tone="error">
          Operation details could not be loaded.{" "}
          <Action onPress={retry} isDisabled={pending}>
            Retry
          </Action>
        </Notice>
      )}
      {detail && <DetailContent detail={detail} onNavigate={onNavigate} />}
    </section>
  );
}

function DetailContent({
  detail,
  onNavigate,
}: {
  detail: HistoryDetail;
  onNavigate: (jobRef: string) => void;
}) {
  const { summary, workflow, followUps, effects } = detail;
  const predecessor = summary?.predecessorJobRef;
  const plan =
    workflow.planVersions === null
      ? "Unavailable"
      : workflow.planVersions.length === 0
        ? "No plan lineage"
        : workflow.planVersions.join(", ");
  return (
    <>
      {(detail.completeness === "partial" ||
        detail.missingReasons.length > 0) && (
        <Notice>
          Some operation information is unavailable.{" "}
          {detail.missingReasons.map(missingLabel).join(" ")}
        </Notice>
      )}
      <Heading level={3}>Operation summary</Heading>
      {summary ? (
        <Readout label="Operation summary" rows={summaryRows(summary)} />
      ) : (
        <Text>Operation summary unavailable.</Text>
      )}
      {predecessor && (
        <div>
          <Action onPress={() => onNavigate(predecessor)}>
            Open previous operation
          </Action>
        </div>
      )}
      <Heading level={3}>Plans and approval</Heading>
      <Readout
        label="Plans and approval"
        rows={[
          { label: "Plan lineage", value: plan },
          {
            label: "Executed plan",
            value: workflow.executedPlan ?? "Unavailable",
          },
          {
            label: "Approval check",
            value: workflow.approvalCheck ?? "Unavailable",
          },
        ]}
      />
      <Heading level={3}>Effects</Heading>
      <Readout
        label="Effects"
        rows={[
          { label: "Estimated", value: effect(effects.estimated) },
          { label: "Executed", value: effect(effects.executed) },
          { label: "Confirmed", value: effect(effects.confirmed) },
        ]}
      />
      <Heading level={3}>Follow-up operations</Heading>
      <FollowUps rows={followUps} onNavigate={onNavigate} />
    </>
  );
}

function FollowUps({
  rows,
  onNavigate,
}: {
  rows: HistoryDetail["followUps"];
  onNavigate: (jobRef: string) => void;
}) {
  if (rows === null) return <Text>Follow-up information unavailable.</Text>;
  if (rows.length === 0) return <Text>No follow-up operations.</Text>;
  return (
    <Readout
      label="Follow-up operations"
      rows={rows.map((item, index) => {
        const job = item.jobRef;
        const facts = [
          item.relation === null
            ? "Relation unavailable"
            : relationLabel(item.relation),
          item.state === null ? "State unavailable" : stateLabel(item.state),
          time(item.finishedAtMs),
        ].join(" · ");
        return {
          label: `Follow-up ${index + 1}`,
          value: (
            <div>
              <Text>{facts}</Text>
              {job && (
                <Action onPress={() => onNavigate(job)}>
                  Open follow-up {index + 1}
                </Action>
              )}
            </div>
          ),
        };
      })}
    />
  );
}

function summaryRows(summary: NonNullable<HistoryDetail["summary"]>) {
  return [
    {
      label: "Type",
      value:
        summary.kindId === "scan.metadata" ? "Disk analysis" : summary.kindId,
    },
    { label: "State", value: stateLabel(summary.state) },
    { label: "Scope", value: summary.scopeSummary ?? "Unavailable" },
    { label: "Started", value: time(summary.startedAtMs) },
    { label: "Finished", value: time(summary.finishedAtMs) },
    { label: "Completed", value: summary.counts?.completed ?? "Unavailable" },
    { label: "Failed", value: summary.counts?.failed ?? "Unavailable" },
    { label: "Skipped", value: summary.counts?.skipped ?? "Unavailable" },
    {
      label: "Overwritten",
      value: summary.counts?.overwritten ?? "Unavailable",
    },
    {
      label: "Unprocessed",
      value: summary.counts?.unprocessed ?? "Unavailable",
    },
  ];
}

function time(value: number | null) {
  if (value === null) return "Time unavailable";
  const date = new Date(value);
  return Number.isFinite(date.getTime())
    ? date.toLocaleString()
    : "Time unavailable";
}

function effect(value: HistoryDetail["effects"]["estimated"]) {
  return value === null
    ? "Unavailable"
    : `${value.value} ${value.unit} · ${value.uncertainty}`;
}
