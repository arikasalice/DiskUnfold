import {
  Action,
  DetailDisclosure,
  Heading,
  Notice,
  Progress,
  Readout,
  StatusBadge,
  Text,
} from "../../components";
import type { AnalysisClient, HistoryDetail } from "../../ipc";
import { useHistoryDetail } from "../../state/history-detail";
import {
  displayTime,
  kindLabel,
  missingLabel,
  relationLabel,
  stateLabel,
  stateTone,
} from "./history-labels";
import { HistoryItemSection } from "./HistoryItems";

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
    <section
      className="du-history-screen"
      aria-labelledby="history-detail-heading"
    >
      <header className="du-history-header">
        <Heading level={2} id="history-detail-heading">
          Operation details
        </Heading>
        <Action variant="quiet" icon="back" autoFocus onPress={onClose}>
          Back to history
        </Action>
      </header>
      <div
        className="du-history-detail"
        role="region"
        aria-label="Recorded operation details"
        tabIndex={0}
      >
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
        {detail && (
          <>
            <OperationSummary detail={detail} />
            <RelatedRecords detail={detail} onNavigate={onNavigate} />
            {detail.summary?.kindId !== "scan.metadata" && (
              <HistoryItemSection client={client} jobRef={jobRef} />
            )}
            <RecordedEvidence detail={detail} />
          </>
        )}
      </div>
    </section>
  );
}

function OperationSummary({ detail }: { detail: HistoryDetail }) {
  const { summary } = detail;
  return (
    <section className="du-history-summary" aria-label="Operation summary">
      {summary ? (
        <>
          <div className="du-history-summary-title">
            <Heading level={3}>{kindLabel(summary.kindId)}</Heading>
            <StatusBadge tone={stateTone(summary.state)}>
              {stateLabel(summary.state)}
            </StatusBadge>
          </div>
          <Readout
            label="Recorded operation"
            rows={[
              {
                label: "Recorded target",
                value: summary.scopeSummary ?? "Unavailable",
              },
              { label: "Finished", value: displayTime(summary.finishedAtMs) },
              { label: "Started", value: displayTime(summary.startedAtMs) },
            ]}
          />
        </>
      ) : (
        <Text>Operation summary unavailable.</Text>
      )}
      {(detail.completeness === "partial" ||
        detail.missingReasons.length > 0) && (
        <Notice>
          Some operation information is unavailable.{" "}
          {[...new Set(detail.missingReasons.map(missingLabel))].join(" ")}
        </Notice>
      )}
    </section>
  );
}

function RelatedRecords({
  detail,
  onNavigate,
}: {
  detail: HistoryDetail;
  onNavigate: (jobRef: string) => void;
}) {
  const { summary, followUps } = detail;
  const predecessor = summary?.predecessorJobRef;
  const relation = summary?.relation;
  return (
    <section className="du-history-related" aria-label="Related records">
      <Heading level={3}>Related records</Heading>
      {relation && (
        <Text kind="muted">Follow-up type · {relationLabel(relation)}</Text>
      )}
      {predecessor && (
        <div>
          <Action variant="quiet" onPress={() => onNavigate(predecessor)}>
            View previous record
          </Action>
        </div>
      )}
      {relation && !predecessor && (
        <Text kind="muted">Previous record unavailable.</Text>
      )}
      {followUps === null && (
        <Text kind="muted">Follow-up information unavailable.</Text>
      )}
      {followUps?.length === 0 && (
        <Text kind="muted">No follow-up operations.</Text>
      )}
      {followUps?.map(({ jobRef, relation, state, finishedAtMs }, index) => {
        const type =
          relation === null ? "Relation unavailable" : relationLabel(relation);
        const status = state === null ? "State unavailable" : stateLabel(state);
        const time = displayTime(finishedAtMs);
        return (
          <div className="du-history-related-row" key={jobRef ?? index}>
            <div>
              <Text>{type}</Text>
              <Text kind="muted">
                {status} · {time}
              </Text>
            </div>
            {jobRef && (
              <Action
                variant="quiet"
                aria-label={
                  "View " +
                  type.toLowerCase() +
                  " record, " +
                  status +
                  ", " +
                  time
                }
                onPress={() => onNavigate(jobRef)}
              >
                View record
              </Action>
            )}
          </div>
        );
      })}
    </section>
  );
}

function RecordedEvidence({ detail }: { detail: HistoryDetail }) {
  const { summary, workflow, followUps, effects } = detail;
  const counts = summary?.counts;
  const unconfirmed = followUps?.every(
    (item) => item.relation && item.relation !== "effect_check",
  );
  return (
    <DetailDisclosure title="Recorded evidence">
      {summary &&
        kindLabel(summary.kindId) === "Operation type unavailable" && (
          <Readout
            label="Recorded type"
            rows={[{ label: "Operation type code", value: summary.kindId }]}
          />
        )}
      {counts ? (
        <section className="du-history-evidence-section">
          <Heading level={3}>Result counts</Heading>
          <Readout
            label="Result counts"
            rows={[
              { label: "Completed", value: counts.completed },
              { label: "Overwritten", value: counts.overwritten },
              { label: "Failed", value: counts.failed },
              { label: "Skipped", value: counts.skipped },
              { label: "Unprocessed", value: counts.unprocessed },
            ]}
          />
        </section>
      ) : (
        <Text kind="muted">Result counts unavailable.</Text>
      )}
      {summary && (
        <Readout
          label="Session information"
          rows={[
            {
              label: "Restore information in this session",
              value: sessionInfo(summary.sessionRestoreAvailable),
            },
          ]}
        />
      )}
      <section className="du-history-evidence-section">
        <Heading level={3}>Plans and approval</Heading>
        <Readout
          label="Plans and approval"
          rows={[
            {
              label: "Plan lineage",
              value:
                workflow.planVersions?.join(", ") ||
                (workflow.planVersions === null
                  ? "Unavailable"
                  : "No plan lineage"),
            },
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
      </section>
      <section className="du-history-evidence-section">
        <Heading level={3}>Effects</Heading>
        <Readout
          label="Effects"
          rows={[
            { label: "Estimated", value: effect(effects.estimated) },
            { label: "Executed", value: effect(effects.executed) },
            {
              label: "Confirmed",
              value: effect(
                effects.confirmed,
                unconfirmed ? "Not yet confirmed" : "Unavailable",
              ),
            },
          ]}
        />
      </section>
    </DetailDisclosure>
  );
}

function sessionInfo(value: boolean | null) {
  if (value === null) return "Unavailable";
  return value ? "Information available" : "No restore information";
}

function effect(
  value: HistoryDetail["effects"]["estimated"],
  fallback = "Unavailable",
) {
  return value === null
    ? fallback
    : value.value + " " + value.unit + " · " + value.uncertainty;
}
