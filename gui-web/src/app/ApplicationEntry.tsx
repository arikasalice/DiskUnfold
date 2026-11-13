import { App } from "./App";
import { DesktopLocale } from "./DesktopLocale";
import type { AnalysisClient } from "../ipc";
import { StartupStatusScreen } from "../features/startup/StartupStatusScreen";
import { ClosingGuard } from "../features/shutdown/ClosingGuard";

/** Selects the GUI entry view from capabilities injected by the native host. */
export function ApplicationEntry({
  client,
  localeEnabled,
  startupStatusEnabled,
  analysisEnabled,
}: {
  client: AnalysisClient;
  localeEnabled: boolean;
  startupStatusEnabled: boolean;
  analysisEnabled: boolean;
}) {
  if (localeEnabled) {
    return (
      <DesktopLocale client={client} analysisEnabled={analysisEnabled}>
        {startupStatusEnabled ? (
          <StartupStatusScreen client={client} />
        ) : undefined}
      </DesktopLocale>
    );
  }
  return startupStatusEnabled ? (
    <ClosingGuard client={client}>
      <StartupStatusScreen client={client} />
    </ClosingGuard>
  ) : (
    <App />
  );
}
