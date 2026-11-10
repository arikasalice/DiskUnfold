import diskUnfoldIcon from "./assets/diskunfold-icon.svg";
import { useEffect, useMemo, useRef, useState } from "react";
import { Action, BrandMark, Heading } from "../components";
import { AnalysisPanel } from "../features/analysis/AnalysisPanel";
import { FileInspection } from "../features/analysis/FileInspection";
import { BrowsePanel } from "../features/browse/BrowsePanel";
import { HistoryPanel } from "../features/history/HistoryPanel";
import { createAnalysisClient, hasWindowsAnalysisBridge } from "../ipc";
import type { AnalysisClient } from "../ipc";
import { createBrowsePort } from "./browse-port";

const desktop = createAnalysisClient();

export function App({ analysis }: { analysis?: AnalysisClient } = {}) {
  const client = analysis ?? (hasWindowsAnalysisBridge() ? desktop : undefined);
  const [history, setHistory] = useState(false);
  const [browseVersion, setBrowseVersion] = useState<string>();
  const browseTrigger = useRef<HTMLButtonElement>(null);
  const restoreBrowseFocus = useRef(false);
  const browsePort = useMemo(
    () => (client && browseVersion ? createBrowsePort(client) : undefined),
    [client, browseVersion],
  );
  useEffect(() => () => browsePort?.dispose(), [browsePort]);
  const opener = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  useEffect(() => {
    if (!history && restoreFocus.current) {
      opener.current?.focus();
      restoreFocus.current = false;
    }
  }, [history]);
  useEffect(() => {
    if (!browseVersion && restoreBrowseFocus.current) {
      browseTrigger.current?.focus();
      restoreBrowseFocus.current = false;
    }
  }, [browseVersion]);
  const analysisOpen = client && !history && browseVersion === undefined;
  return (
    <main
      className="du-app"
      data-analysis-open={Boolean(analysisOpen) || undefined}
    >
      <header className="du-app-header">
        <div className="du-brand">
          <BrandMark src={diskUnfoldIcon} />
          <Heading level={1}>DiskUnfold</Heading>
        </div>
        <div className="du-app-header__actions">
          {client && !history && !browseVersion && (
            <Action
              variant="quiet"
              icon="history"
              ref={opener}
              onPress={() => setHistory(true)}
            >
              Operation history
            </Action>
          )}
        </div>
      </header>
      {client && (
        <div className="du-analysis-slot" hidden={!analysisOpen}>
          <AnalysisPanel
            client={client}
            onBrowse={setBrowseVersion}
            browseTrigger={browseTrigger}
          />
        </div>
      )}
      {client && browsePort && browseVersion && (
        <div className="du-browse-shell">
          <Action
            variant="quiet"
            icon="back"
            autoFocus
            onPress={() => {
              restoreBrowseFocus.current = true;
              setBrowseVersion(undefined);
            }}
          >
            Back to analysis
          </Action>
          <BrowsePanel
            port={browsePort}
            resultVersion={browseVersion}
            inspectFile={(item) => (
              <FileInspection
                key={`${item.resultVersion}:${item.itemNo}`}
                client={client}
                item={item}
              />
            )}
          />
        </div>
      )}
      {client && history && (
        <HistoryPanel
          client={client}
          onClose={() => {
            restoreFocus.current = true;
            setHistory(false);
          }}
        />
      )}
    </main>
  );
}
