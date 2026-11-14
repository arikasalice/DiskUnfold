import { useEffect, useMemo } from "react";
import type { SizeDisplay } from "../i18n/bytes";
import { useLocaleText } from "../i18n/presentation";
import { FileInspection } from "../features/analysis/FileInspection";
import { BrowsePanel } from "../features/browse/BrowsePanel";
import { SampleExperience } from "../features/sample";
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
  return (
    <BrowsePanel
      key={resultVersion}
      port={port}
      resultVersion={resultVersion}
      display={display}
      rootLabel={text("ui.sample.root_name", "Sample data")}
      inspectItem={(item, kind) => (
        <FileInspection
          key={`${item.resultVersion}:${item.itemNo}`}
          client={client}
          item={item}
          itemKind={kind}
          display={display}
        />
      )}
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
    ReturnType<typeof useSampleFlow>,
    "sample" | "endSample" | "startRealAnalysis"
  >;
  display: SizeDisplay;
}) {
  const { sample } = flow;
  if (!client) return null;
  return (
    <SampleExperience
      session={sample.session}
      pending={sample.pending}
      problem={sample.problem}
      onReset={sample.reset}
      onClose={flow.endSample}
      onRealAnalysis={flow.startRealAnalysis}
      onDismiss={sample.dismiss}
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
