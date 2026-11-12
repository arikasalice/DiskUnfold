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
import { useLocaleText } from "../../i18n/presentation";
import { useHistoryDetail } from "../../state/history-detail";
import {
  useHistoryTime,
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
  const text = useLocaleText();
  const { detail, pending, error, retry } = useHistoryDetail(client, jobRef);
  return (
    <section
      className="du-history-screen"
      aria-labelledby="history-detail-heading"
    >
      <header className="du-history-header">
        <Heading level={2} id="history-detail-heading">
          {text("ui.history.operation_details", "Operation details")}
        </Heading>
        <Action variant="quiet" icon="back" autoFocus onPress={onClose}>
          {text("ui.history.back_to_history", "Back to history")}
        </Action>
      </header>
      <div
        className="du-history-detail"
        role="region"
        aria-label={text(
          "ui.history.recorded_operation_details",
          "Recorded operation details",
        )}
        tabIndex={0}
      >
        {pending && (
          <Progress
            label={text(
              "ui.history.loading_operation_details",
              "Loading operation details",
            )}
          />
        )}
        {error === "removed" && (
          <Notice>
            {text(
              "ui.history.removed",
              "History was removed and can no longer be viewed.",
            )}
          </Notice>
        )}
        {error === "storage" && (
          <Notice tone="error">
            {text(
              "ui.history.operation_details_load_failed",
              "Operation details could not be loaded.",
            )}{" "}
            <Action onPress={retry} isDisabled={pending}>
              {text("ui.history.retry", "Retry")}
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
  const text = useLocaleText();
  const displayTime = useHistoryTime();
  const { summary } = detail;
  return (
    <section
      className="du-history-summary"
      aria-label={text("ui.history.operation_summary", "Operation summary")}
    >
      {summary ? (
        <>
          <div className="du-history-summary-title">
            <Heading level={3}>{kindLabel(summary.kindId, text)}</Heading>
            <StatusBadge tone={stateTone(summary.state)}>
              {stateLabel(summary.state, false, text)}
            </StatusBadge>
          </div>
          <Readout
            label={text("ui.history.recorded_operation", "Recorded operation")}
            rows={[
              {
                label: text("ui.history.recorded_target", "Recorded target"),
                value:
                  summary.scopeSummary ??
                  text("ui.history.unavailable", "Unavailable"),
              },
              {
                label: text("ui.history.finished", "Finished"),
                value: displayTime(summary.finishedAtMs),
              },
              {
                label: text("ui.history.started", "Started"),
                value: displayTime(summary.startedAtMs),
              },
            ]}
          />
        </>
      ) : (
        <Text>
          {text(
            "ui.history.operation_summary_unavailable",
            "Operation summary unavailable.",
          )}
        </Text>
      )}
      {(detail.completeness === "partial" ||
        detail.missingReasons.length > 0) && (
        <Notice>
          {text(
            "ui.history.operation_information_unavailable",
            "Some operation information is unavailable.",
          )}{" "}
          {[
            ...new Set(
              detail.missingReasons.map((reason) => missingLabel(reason, text)),
            ),
          ].join(" ")}
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
  const text = useLocaleText();
  const { summary, followUps } = detail;
  const displayTime = useHistoryTime();
  const predecessor = summary?.predecessorJobRef;
  const relation = summary?.relation;
  return (
    <section
      className="du-history-related"
      aria-label={text("ui.history.related_records", "Related records")}
    >
      <Heading level={3}>
        {text("ui.history.related_records", "Related records")}
      </Heading>
      {relation && (
        <Text kind="muted">
          {text("ui.history.follow_up_type", "Follow-up type · {type}", {
            type: relationLabel(relation, text),
          })}
        </Text>
      )}
      {predecessor && (
        <div>
          <Action variant="quiet" onPress={() => onNavigate(predecessor)}>
            {text("ui.history.view_previous_record", "View previous record")}
          </Action>
        </div>
      )}
      {relation && !predecessor && (
        <Text kind="muted">
          {text(
            "ui.history.previous_record_unavailable",
            "Previous record unavailable.",
          )}
        </Text>
      )}
      {followUps === null && (
        <Text kind="muted">
          {text(
            "ui.history.follow_up_unavailable",
            "Follow-up information unavailable.",
          )}
        </Text>
      )}
      {followUps?.length === 0 && (
        <Text kind="muted">
          {text("ui.history.no_follow_ups", "No follow-up operations.")}
        </Text>
      )}
      {followUps?.map(({ jobRef, relation, state, finishedAtMs }, index) => {
        const type =
          relation === null
            ? text("ui.history.relation_unavailable", "Relation unavailable")
            : relationLabel(relation, text);
        const status =
          state === null
            ? text("ui.history.state_unavailable", "State unavailable")
            : stateLabel(state, false, text);
        const time = displayTime(finishedAtMs);
        return (
          <div className="du-history-related-row" key={jobRef ?? index}>
            <div>
              <Text>{type}</Text>
              <Text kind="muted">
                {text("ui.history.status_time", "{status} · {time}", {
                  status,
                  time,
                })}
              </Text>
            </div>
            {jobRef && (
              <Action
                variant="quiet"
                aria-label={text(
                  "ui.history.view_related_record_aria",
                  "View {type} record, {status}, {time}",
                  { type: type.toLowerCase(), status, time },
                )}
                onPress={() => onNavigate(jobRef)}
              >
                {text("ui.history.view_record", "View record")}
              </Action>
            )}
          </div>
        );
      })}
    </section>
  );
}

function RecordedEvidence({ detail }: { detail: HistoryDetail }) {
  const text = useLocaleText();
  const { summary, workflow, effects } = detail;
  const counts = summary?.counts;
  return (
    <DetailDisclosure
      title={text("ui.history.recorded_evidence", "Recorded evidence")}
    >
      {summary &&
        summary.kindId !== "scan.metadata" &&
        summary.kindId !== "duplicate.check" && (
          <Readout
            label={text("ui.history.recorded_type", "Recorded type")}
            rows={[
              {
                label: text(
                  "ui.history.operation_type_code",
                  "Operation type code",
                ),
                value: summary.kindId,
              },
            ]}
          />
        )}
      {counts ? (
        <section className="du-history-evidence-section">
          <Heading level={3}>
            {text("ui.history.result_counts", "Result counts")}
          </Heading>
          <Readout
            label={text("ui.history.result_counts", "Result counts")}
            rows={[
              {
                label: text("ui.history.outcome_completed", "Completed"),
                value: counts.completed,
              },
              {
                label: text("ui.history.outcome_overwritten", "Overwritten"),
                value: counts.overwritten,
              },
              {
                label: text("ui.history.outcome_failed", "Failed"),
                value: counts.failed,
              },
              {
                label: text("ui.history.outcome_skipped", "Skipped"),
                value: counts.skipped,
              },
              {
                label: text("ui.history.outcome_unprocessed", "Unprocessed"),
                value: counts.unprocessed,
              },
            ]}
          />
        </section>
      ) : (
        <Text kind="muted">
          {text(
            "ui.history.result_counts_unavailable",
            "Result counts unavailable.",
          )}
        </Text>
      )}
      {summary && (
        <Readout
          label={text("ui.history.session_information", "Session information")}
          rows={[
            {
              label: text(
                "ui.history.restore_information_session",
                "Restore information in this session",
              ),
              value: sessionInfo(summary.sessionRestoreAvailable, text),
            },
          ]}
        />
      )}
      <section className="du-history-evidence-section">
        <Heading level={3}>
          {text("ui.history.plans_approval", "Plans and approval")}
        </Heading>
        <Readout
          label={text("ui.history.plans_approval", "Plans and approval")}
          rows={[
            {
              label: text("ui.history.plan_lineage", "Plan lineage"),
              value:
                workflow.planVersions?.join(", ") ||
                (workflow.planVersions === null
                  ? text("ui.history.unavailable", "Unavailable")
                  : text("ui.history.no_plan_lineage", "No plan lineage")),
            },
            {
              label: text("ui.history.executed_plan", "Executed plan"),
              value:
                workflow.executedPlan ??
                text("ui.history.unavailable", "Unavailable"),
            },
            {
              label: text("ui.history.approval_check", "Approval check"),
              value:
                workflow.approvalCheck ??
                text("ui.history.unavailable", "Unavailable"),
            },
          ]}
        />
      </section>
      <section className="du-history-evidence-section">
        <Heading level={3}>{text("ui.history.effects", "Effects")}</Heading>
        <Readout
          label={text("ui.history.effects", "Effects")}
          rows={[
            {
              label: text("ui.history.estimated", "Estimated"),
              value: effect(effects.estimated, text),
            },
            {
              label: text("ui.history.executed", "Executed"),
              value: effect(effects.executed, text),
            },
            {
              label: text("ui.history.confirmed", "Confirmed"),
              value: effect(effects.confirmed, text),
            },
          ]}
        />
      </section>
    </DetailDisclosure>
  );
}

function sessionInfo(
  value: boolean | null,
  text: ReturnType<typeof useLocaleText>,
) {
  if (value === null) return text("ui.history.unavailable", "Unavailable");
  return value
    ? text("ui.history.information_available", "Information available")
    : text("ui.history.no_restore_information", "No restore information");
}

function effect(
  value: HistoryDetail["effects"]["estimated"],
  text: ReturnType<typeof useLocaleText>,
) {
  return value === null
    ? text("ui.history.unavailable", "Unavailable")
    : value.value + " " + value.unit + " · " + value.uncertainty;
}
