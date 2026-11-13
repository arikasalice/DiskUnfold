import { useCallback, useMemo } from "react";
import {
  Action,
  Notice,
  Progress,
  Readout,
  ReviewSection,
  StatusBadge,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { AnalysisClient, SupportReport } from "../../ipc";
import { useHostQuery } from "../../state/use-host-query";
import { CombinationTable } from "./CombinationTable";
import { FeatureAreas } from "./FeatureAreas";
import {
  cpuLabel,
  osLabel,
  reasonText,
  securityKindLabel,
  statusLabel,
  statusNote,
  supportCopy,
} from "./support-copy";
import type { SupportCopy } from "./support-copy";
import "../../theme/info-screens.css";

function statusTone(status: string) {
  if (status === "officially_verified") return "good" as const;
  if (status === "conditional") return "warning" as const;
  if (status === "unsupported_found") return "danger" as const;
  if (status === "not_verified") return "info" as const;
  return "neutral" as const;
}

function SecuritySupport({
  report,
  copy,
}: {
  report: SupportReport;
  copy: SupportCopy;
}) {
  const basis = report.securitySupport;
  return (
    <ReviewSection title={copy.securityHeading}>
      <Text kind="muted">{copy.securityIntro}</Text>
      {basis === null ? (
        <Notice tone="info">{copy.securityNone}</Notice>
      ) : (
        <Readout
          label={copy.securityHeading}
          rows={[
            {
              label: copy.securityKind,
              value: securityKindLabel(basis.kind, copy),
            },
            {
              label: copy.securitySource,
              value: basis.source ?? copy.notDetected,
              wideValue: true,
            },
            {
              label: copy.securityChecked,
              value: basis.checkedOn ?? copy.notDetected,
            },
          ]}
        />
      )}
    </ReviewSection>
  );
}

/** Support-table header and current-system sections, with guidance for the unusual states. */
function Environment({
  report,
  copy,
  matrixNotice,
  envUnknown,
}: {
  report: SupportReport;
  copy: SupportCopy;
  matrixNotice: string | undefined;
  envUnknown: boolean;
}) {
  const { environment: env } = report;
  return (
    <>
      <ReviewSection title={copy.tableHeading}>
        {matrixNotice && <Notice tone="info">{matrixNotice}</Notice>}
        <Readout
          label={copy.tableHeading}
          rows={[
            {
              label: copy.matrixRevision,
              value: report.matrixRevision ?? copy.matrixMissing,
            },
            {
              label: copy.matrixAsOf,
              value: report.asOf ?? copy.matrixMissing,
            },
          ]}
        />
      </ReviewSection>
      <ReviewSection title={copy.envHeading}>
        {envUnknown && <Notice tone="info">{copy.envUnknown}</Notice>}
        <Readout
          label={copy.envHeading}
          rows={[
            { label: copy.envOs, value: osLabel(env.osFamily, copy) },
            { label: copy.envRelease, value: env.release ?? copy.notDetected },
            { label: copy.envEdition, value: env.edition ?? copy.notDetected },
            { label: copy.envCpu, value: cpuLabel(env.cpu, copy) },
          ]}
        />
      </ReviewSection>
    </>
  );
}

/**
 * Support information (F15 FR6.HD2.gui-web.DD1; IF-04).
 *
 * The header shows the support-table revision and date. The combination
 * status, the security-support basis and the per-area status sit in separate
 * sections so one is never read as another.
 */
export function SupportScreen({ client }: { client: AnalysisClient }) {
  const text = useLocaleText();
  const copy = useMemo(() => supportCopy(text), [text]);
  const load = useCallback(() => client.call("app.support_info", {}), [client]);
  const { data, loading, failed, reload } = useHostQuery(load);

  if (!data) {
    return (
      <div className="du-info-screen">
        {loading && <Progress label={copy.loading} />}
        {failed && (
          <>
            <Notice tone="error">{copy.loadFailed}</Notice>
            <div className="du-info-actions">
              <Action variant="primary" onPress={reload}>
                {copy.retry}
              </Action>
            </div>
          </>
        )}
      </div>
    );
  }

  const matrixInvalid = data.reasonKey === "support.matrix_invalid";
  const matrixMissing =
    data.matrixRevision === null ||
    data.reasonKey === "support.matrix_unavailable";
  const matrixNotice = matrixInvalid
    ? copy.reasonMatrixInvalid
    : matrixMissing
      ? copy.matrixUnavailable
      : undefined;
  const envUnknown =
    data.environment.osFamily === "unknown" ||
    data.environment.cpu === "unknown" ||
    data.reasonKey === "support.env_unknown";
  const guided = matrixNotice !== undefined || envUnknown;
  return (
    <div className="du-info-screen">
      <Text kind="muted">{copy.intro}</Text>
      {failed && <Notice tone="error">{copy.refreshFailed}</Notice>}
      <Environment
        report={data}
        copy={copy}
        matrixNotice={matrixNotice}
        envUnknown={envUnknown}
      />
      <ReviewSection title={copy.combinationHeading}>
        <div className="du-info-badges">
          <Text kind="eyebrow">{copy.combinationLabel}</Text>
          <StatusBadge tone={statusTone(data.combinationStatus)}>
            {statusLabel(data.combinationStatus, copy)}
          </StatusBadge>
        </div>
        <Text>{statusNote(data.combinationStatus, copy)}</Text>
        {data.reasonKey && !guided && (
          <Notice tone="info">{reasonText(data.reasonKey, copy)}</Notice>
        )}
      </ReviewSection>
      <SecuritySupport report={data} copy={copy} />
      <ReviewSection title={copy.featuresHeading}>
        <FeatureAreas features={data.features} copy={copy} />
      </ReviewSection>
      {matrixNotice === undefined && (
        <ReviewSection title={copy.combinationsHeading}>
          <CombinationTable combinations={data.combinations} copy={copy} />
        </ReviewSection>
      )}
      <div className="du-info-actions">
        <Action icon="refresh" isPending={loading} onPress={reload}>
          {copy.refresh}
        </Action>
      </div>
    </div>
  );
}
