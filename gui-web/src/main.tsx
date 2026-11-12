import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { DesktopLocale } from "./app/DesktopLocale";
import {
  createAnalysisClient,
  hasDesktopLocaleBridge,
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
createRoot(root).render(
  hasDesktopLocaleBridge() ? (
    <DesktopLocale
      client={createAnalysisClient()}
      analysisEnabled={hasWindowsAnalysisBridge()}
    />
  ) : (
    <App />
  ),
);
