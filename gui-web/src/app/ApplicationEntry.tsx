import { App } from "./App";
import { DesktopLocale } from "./DesktopLocale";
import type { AnalysisClient } from "../ipc";
import { StartupStatusScreen } from "../features/startup/StartupStatusScreen";
import { ClosingGuard } from "../features/shutdown/ClosingGuard";
import { InfoEntry } from "./InfoEntry";

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
      <DesktopLocale
        client={client}
        analysisEnabled={analysisEnabled}
        startupBanner={startupStatusEnabled && analysisEnabled}
      >
        {startupStatusEnabled && !analysisEnabled ? (
          <>
            <StartupStatusScreen client={client} />
            <InfoEntry client={client} standalone />
          </>
        ) : undefined}
      </DesktopLocale>
    );
  }
  return startupStatusEnabled ? (
    <ClosingGuard client={client}>
      <StartupStatusScreen client={client} />
      <InfoEntry client={client} standalone />
    </ClosingGuard>
  ) : (
    <App />
  );
}
