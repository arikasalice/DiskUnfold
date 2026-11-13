import { useCallback } from "react";
import {
  Action,
  DetailDisclosure,
  Notice,
  Progress,
  Text,
  TextDocument,
} from "../../components";
import type { AnalysisClient, NoticeKind } from "../../ipc";
import { useHostQuery } from "../../state/use-host-query";
import type { ProductCopy } from "./product-copy";

function NoticeBody({
  client,
  kind,
  copy,
}: {
  client: AnalysisClient;
  kind: NoticeKind;
  copy: ProductCopy;
}) {
  const load = useCallback(
    () => client.call("app.notices", { kind }),
    [client, kind],
  );
  const { data, loading, failed, reload } = useHostQuery(load);
  return (
    <div className="du-info-notice">
      {loading && <Progress label={copy.noticeLoading} />}
      {failed && (
        <>
          <Notice tone="error">{copy.noticeFailed}</Notice>
          <div className="du-info-actions">
            <Action onPress={reload}>{copy.retry}</Action>
          </div>
        </>
      )}
      {data && data.text === null && (
        <Notice tone="info">{copy.noticeEmpty}</Notice>
      )}
      {data && data.text !== null && (
        <>
          <TextDocument label={copy.noticeRegionNamed(copy.noticeNames[kind])}>
            {data.text}
          </TextDocument>
          <Text kind="muted">
            {data.sourceCommit
              ? copy.noticeCommitValue(data.sourceCommit)
              : copy.noticeCommitUnknown}
          </Text>
        </>
      )}
    </div>
  );
}

/** One notice document. The text is read only when the user opens it (IF-02). */
export function NoticeViewer({
  client,
  kind,
  available,
  copy,
}: {
  client: AnalysisClient;
  kind: NoticeKind;
  available: boolean;
  copy: ProductCopy;
}) {
  const name = Object.hasOwn(copy.noticeNames, kind)
    ? copy.noticeNames[kind]
    : kind;
  if (!available) {
    return <Text>{copy.noticeMissingNamed(name)}</Text>;
  }
  return (
    <DetailDisclosure title={name}>
      <NoticeBody client={client} kind={kind} copy={copy} />
    </DetailDisclosure>
  );
}
