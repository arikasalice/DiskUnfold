import diskUnfoldIcon from "./assets/diskunfold-icon.svg";
import { BrandMark, Heading, StatusBadge, Text } from "../components";
import { AnalysisPanel } from "../features/analysis/AnalysisPanel";
import { createAnalysisClient, hasWindowsAnalysisBridge } from "../ipc";
import type { AnalysisClient } from "../ipc";

const desktop = createAnalysisClient();

export function App({ analysis }: { analysis?: AnalysisClient } = {}) {
  const client = analysis ?? (hasWindowsAnalysisBridge() ? desktop : undefined);
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
        <StatusBadge tone="good">Local analysis</StatusBadge>
      </header>
      {client && <AnalysisPanel client={client} />}
    </main>
  );
}
