import { createRoot } from "react-dom/client";
import { ApplicationEntry } from "./app/ApplicationEntry";
import {
  createAnalysisClient,
  hasDesktopLocaleBridge,
  hasStartupStatusBridge,
  hasWindowsAnalysisBridge,
} from "./ipc";
import "./theme/tokens.css";
import "./theme/workspace.css";
import "./theme/analysis-workspace.css";
import "./theme/drive-picker.css";

const root = document.getElementById("root");
if (root === null) {
  throw new Error("Missing app root");
}
const client = createAnalysisClient();
const startupBridge = hasStartupStatusBridge();
createRoot(root).render(
  <ApplicationEntry
    client={client}
    localeEnabled={hasDesktopLocaleBridge()}
    startupStatusEnabled={startupBridge}
    analysisEnabled={hasWindowsAnalysisBridge()}
  />,
);
