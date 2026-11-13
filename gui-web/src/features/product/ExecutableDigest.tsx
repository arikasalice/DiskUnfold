import { useCallback, useEffect, useRef, useState } from "react";
import { Action, LiveStatus, Notice, Progress, Text } from "../../components";
import { newCorrelationId } from "../../ipc";
import type { AnalysisClient } from "../../ipc";
import type { ProductCopy } from "./product-copy";

type Digest =
  | { phase: "idle" }
  | { phase: "computing" }
  | { phase: "done"; sha256: string }
  | { phase: "unavailable"; reasonKey: string | null }
  | { phase: "failed" };

/**
 * Requests the executable SHA-256 (F15 IF-03) and shows it for comparison.
 *
 * Only the response that echoes the newest correlation identifier is applied,
 * and a response that arrives after the screen closed is discarded. The value
 * is never presented as verified.
 */
export function ExecutableDigest({
  client,
  copy,
}: {
  client: AnalysisClient;
  copy: ProductCopy;
}) {
  const [digest, setDigest] = useState<Digest>({ phase: "idle" });
  const [copyResult, setCopyResult] = useState<"copied" | "failed">();
  const current = useRef<string | null>(null);
  useEffect(
    () => () => {
      current.current = null;
    },
    [],
  );

  const request = useCallback(() => {
    const correlationId = newCorrelationId();
    current.current = correlationId;
    setCopyResult(undefined);
    setDigest({ phase: "computing" });
    client.call("app.executable_digest", { correlationId }).then(
      (response) => {
        if (
          current.current !== correlationId ||
          response.correlationId !== correlationId
        )
          return;
        setDigest(
          response.sha256
            ? { phase: "done", sha256: response.sha256 }
            : { phase: "unavailable", reasonKey: response.reasonKey },
        );
      },
      () => {
        if (current.current === correlationId) setDigest({ phase: "failed" });
      },
    );
  }, [client]);

  const sha256 = digest.phase === "done" ? digest.sha256 : undefined;
  const copyDigest = useCallback(() => {
    if (!sha256) return;
    const write = globalThis.navigator?.clipboard?.writeText(sha256);
    if (!write) {
      setCopyResult("failed");
      return;
    }
    write.then(
      () => setCopyResult("copied"),
      () => setCopyResult("failed"),
    );
  }, [sha256]);

  return (
    <div className="du-info-digest">
      <Text kind="muted">{copy.digestIntro}</Text>
      {digest.phase === "computing" && (
        <Progress label={copy.digestComputing} />
      )}
      {digest.phase === "done" && (
        <div className="du-info-digest__result">
          <Text kind="eyebrow">{copy.digestLabel}</Text>
          <Text kind="path">{digest.sha256}</Text>
          <Text kind="muted">{copy.digestCompare}</Text>
          <div className="du-info-actions">
            <Action icon="copy" onPress={copyDigest}>
              {copy.digestCopy}
            </Action>
          </div>
          {copyResult === "copied" && (
            <LiveStatus>{copy.digestCopied}</LiveStatus>
          )}
          {copyResult === "failed" && (
            <Notice tone="error">{copy.digestCopyFailed}</Notice>
          )}
        </div>
      )}
      {digest.phase === "unavailable" && (
        <Notice tone="info">
          {digest.reasonKey === "product.digest_unavailable"
            ? copy.digestUnavailable
            : copy.digestUnavailableOther}
        </Notice>
      )}
      {digest.phase === "failed" && (
        <Notice tone="error">{copy.digestFailed}</Notice>
      )}
      <div className="du-info-actions">
        <Action
          variant={digest.phase === "idle" ? "primary" : "secondary"}
          isPending={digest.phase === "computing"}
          onPress={request}
        >
          {copy.digestAction}
        </Action>
      </div>
    </div>
  );
}
