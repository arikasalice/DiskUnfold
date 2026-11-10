import diskUnfoldIcon from "./assets/diskunfold-icon.svg";
import { useEffect, useRef, useState } from "react";
import { Action, BrandMark, Heading, StatusBadge, Text } from "../components";
import { AnalysisPanel } from "../features/analysis/AnalysisPanel";
import { HistoryPanel } from "../features/history/HistoryPanel";
import { createAnalysisClient, hasWindowsAnalysisBridge } from "../ipc";
import type { AnalysisClient } from "../ipc";

const desktop = createAnalysisClient();

export function App({ analysis }: { analysis?: AnalysisClient } = {}) {
  const client = analysis ?? (hasWindowsAnalysisBridge() ? desktop : undefined);
  const [history, setHistory] = useState(false);
  const opener = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  useEffect(() => {
    if (!history && restoreFocus.current) {
      opener.current?.focus();
      restoreFocus.current = false;
    }
  }, [history]);
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
          {client && !history && (
            <Action ref={opener} onPress={() => setHistory(true)}>
              Operation history
            </Action>
          )}
        </div>
      </header>
      {client && (
        <div hidden={history}>
          <AnalysisPanel client={client} />
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
