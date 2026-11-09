import diskUnfoldIcon from "./assets/diskunfold-icon.svg";
import { AnalysisPanel } from "../features/analysis/AnalysisPanel";
import { createAnalysisClient, hasWindowsAnalysisBridge } from "../ipc";
import type { AnalysisClient } from "../ipc";

const desktop = createAnalysisClient();

export function App({ analysis }: { analysis?: AnalysisClient } = {}) {
  const client = analysis ?? (hasWindowsAnalysisBridge() ? desktop : undefined);
  return (
    <main>
      <img src={diskUnfoldIcon} width={64} height={64} alt="" />
      <h1>DiskUnfold</h1>
      {client && <AnalysisPanel client={client} />}
    </main>
  );
}
