import { useCallback, useMemo } from "react";
import {
  Action,
  Notice,
  Progress,
  Readout,
  ReviewSection,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { AnalysisClient } from "../../ipc";
import { useHostQuery } from "../../state/use-host-query";
import { ExecutableDigest } from "./ExecutableDigest";
import { NoticeViewer } from "./NoticeViewer";
import { buildKindText, installFormText, productCopy } from "./product-copy";
import "../../theme/info-screens.css";

/**
 * Product information (F15 FR5.HD2.gui-web.DD1; IF-01, IF-02, IF-03).
 *
 * Shows what app-host reports about this build. A value the host could not
 * establish is shown as such with a plain-language reason. Nothing on this
 * screen claims that the program file is verified.
 */
export function ProductScreen({ client }: { client: AnalysisClient }) {
  const text = useLocaleText();
  const copy = useMemo(() => productCopy(text), [text]);
  const load = useCallback(() => client.call("app.product_info", {}), [client]);
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

  const missing = (value: string | number | null) =>
    value === null ? copy.missing : String(value);
  const build = buildKindText(data.buildKind, copy);
  const install = installFormText(
    data.installForm,
    data.installFormReasonKey,
    copy,
  );
  const target =
    data.targetOs === null && data.targetCpu === null
      ? copy.missing
      : [data.targetOs, data.targetCpu]
          .map((part) => missing(part))
          .join(" / ");
  return (
    <div className="du-info-screen">
      {failed && <Notice tone="error">{copy.refreshFailed}</Notice>}
      <ReviewSection title={copy.buildHeading}>
        <Readout
          label={copy.buildHeading}
          rows={[
            { label: copy.version, value: missing(data.productVersion) },
            {
              label: copy.buildKind,
              value: build.value,
              notes: build.note ? [build.note] : [],
            },
            {
              label: copy.commit,
              value: missing(data.commit),
              wideValue: true,
            },
            { label: copy.builtAt, value: missing(data.builtAtUtc) },
            { label: copy.target, value: target },
            {
              label: copy.installForm,
              value: install.value,
              notes: install.note ? [install.note] : [],
            },
            {
              label: copy.matrixRevision,
              value: missing(data.supportMatrixRevision),
            },
            { label: copy.matrixAsOf, value: missing(data.supportMatrixAsOf) },
            { label: copy.appDbFormat, value: missing(data.appDbFormat) },
            {
              label: copy.snapshotFormat,
              value: missing(data.snapshotFormat),
            },
          ]}
        />
      </ReviewSection>
      <ReviewSection title={copy.noticesHeading}>
        {data.notices.map((notice) => (
          <NoticeViewer
            key={notice.kind}
            client={client}
            kind={notice.kind}
            available={notice.available}
            copy={copy}
          />
        ))}
      </ReviewSection>
      <ReviewSection title={copy.digestHeading}>
        <ExecutableDigest client={client} copy={copy} />
      </ReviewSection>
      <div className="du-info-actions">
        <Action icon="refresh" isPending={loading} onPress={reload}>
          {copy.refresh}
        </Action>
      </div>
    </div>
  );
}
