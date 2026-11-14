import { useCallback, useEffect, useMemo } from "react";
import type { SizeDisplay } from "../i18n/bytes";
import { useLocaleText } from "../i18n/presentation";
import { ExampleBadge } from "../components";
import { FileInspection } from "../features/analysis/FileInspection";
import { BrowsePanel } from "../features/browse/BrowsePanel";
import { SampleEntry, SampleExperience } from "../features/sample";
import type { AnalysisClient } from "../ipc";
import { createBrowsePort } from "./browse-port";
import type { useSampleFlow } from "./use-sample-flow";

/**
 * Browses one sample result version. It owns its own browse port, so sample
 * folders never share a region, selection or page cache with the real
 * analysis. The sample root appears under its display name.
 */
export function SampleBrowse({
  client,
  resultVersion,
  display,
}: {
  client: AnalysisClient;
  resultVersion: string;
  display: SizeDisplay;
}) {
  const text = useLocaleText();
  const port = useMemo(() => createBrowsePort(client), [client]);
  useEffect(() => () => port.dispose(), [port]);
  // Sample items are not real files, so the actions that act on a path are off.
  const unavailable = text(
    "ui.sample.action_unavailable",
    "Cannot run on sample data",
  );
  return (
    <BrowsePanel
      key={resultVersion}
      port={port}
      resultVersion={resultVersion}
      display={display}
      rootLabel={text("ui.sample.root_name", "Sample data")}
      actionsDisabledReason={unavailable}
      titleBadge={
        <ExampleBadge>
          {text("ui.sample.example_badge", "Example")}
        </ExampleBadge>
      }
      inspectItem={(item, kind) => (
        <FileInspection
          key={`${item.resultVersion}:${item.itemNo}`}
          client={client}
          item={item}
          itemKind={kind}
          display={display}
          actionsDisabledReason={unavailable}
        />
      )}
    />
  );
}

type SampleFlow = ReturnType<typeof useSampleFlow>;

/**
 * The entry button's place on the analysis target screen: the button and what
 * follows its press (progress, an unavailable notice, a failed request).
 */
export function SampleEntrySlot({
  flow,
}: {
  flow: Pick<
    SampleFlow,
    "sample" | "sampleOpen" | "entryRef" | "endSample" | "focusEntry"
  >;
}) {
  const { sample, sampleOpen, entryRef, endSample, focusEntry } = flow;
  const { open, reset, dismiss } = sample;
  const release = useCallback(async () => {
    if (await endSample()) focusEntry();
  }, [endSample, focusEntry]);
  return (
    <SampleEntry
      session={sampleOpen ? undefined : sample.session}
      pending={sample.pending}
      problem={sampleOpen ? undefined : sample.problem}
      preparedItems={sample.preparedItems}
      entryRef={entryRef}
      onStart={() => void open()}
      onRetry={() => void reset()}
      onRelease={() => void release()}
      onDismissProblem={() => {
        dismiss();
        focusEntry();
      }}
    />
  );
}

/** The sample strip, notices and sample result browser for the open session, if any. */
export function SampleContext({
  client,
  flow,
  display,
}: {
  client: AnalysisClient | undefined;
  flow: Pick<
    SampleFlow,
    "sample" | "sampleOpen" | "endSample" | "startRealAnalysis" | "focusEntry"
  >;
  display: SizeDisplay;
}) {
  const { sample } = flow;
  if (!client) return null;
  return (
    <SampleExperience
      session={sample.session}
      reached={sample.reached}
      screen={flow.sampleOpen}
      pending={sample.pending}
      problem={sample.problem}
      onReset={sample.reset}
      onClose={flow.endSample}
      onRealAnalysis={flow.startRealAnalysis}
      onDismiss={sample.dismiss}
      onFocusEntry={flow.focusEntry}
    >
      {(resultVersion) => (
        <SampleBrowse
          client={client}
          resultVersion={resultVersion}
          display={display}
        />
      )}
    </SampleExperience>
  );
}
