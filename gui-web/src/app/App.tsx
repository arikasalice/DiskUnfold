import diskUnfoldIcon from "./assets/diskunfold-icon.svg";
import { useEffect, useMemo, useRef, useState } from "react";
import { Action, BrandMark, Heading, StatusBadge, Text } from "../components";
import { AnalysisPanel } from "../features/analysis/AnalysisPanel";
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
  return (
    <main className="du-app">
      <header className="du-app-header">
        <div className="du-brand">
          <BrandMark src={diskUnfoldIcon} />
          <div>
            <Heading level={1}>DiskUnfold</Heading>
            <Text kind="muted">Disk space workspace</Text>
          </div>
        </div>
        <div className="du-app-header__actions">
          <StatusBadge tone="good">Local analysis</StatusBadge>
          {client && !history && !browseVersion && (
            <Action ref={opener} onPress={() => setHistory(true)}>
              Operation history
            </Action>
          )}
        </div>
      </header>
      {client && (
        <div hidden={history || browseVersion !== undefined}>
          <AnalysisPanel
            client={client}
            onBrowse={setBrowseVersion}
            browseTrigger={browseTrigger}
          />
        </div>
      )}
      {browsePort && browseVersion && (
        <div className="du-analysis">
          <Action
            autoFocus
            onPress={() => {
              restoreBrowseFocus.current = true;
              setBrowseVersion(undefined);
            }}
          >
            Back to analysis
          </Action>
          <BrowsePanel port={browsePort} resultVersion={browseVersion} />
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
