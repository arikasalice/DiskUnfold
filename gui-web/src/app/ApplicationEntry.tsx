import { App } from "./App";
import { useMemo } from "react";
import { TitleBar, TitleBarProvider } from "../components";
import diskUnfoldIcon from "./assets/diskunfold-icon.svg";
import { DesktopLocale } from "./DesktopLocale";
import {
  hasCustomTitlebar,
  requestWindowDrag,
  requestWindowZoom,
} from "../ipc";
import type { AnalysisClient } from "../ipc";
import { StartupStatusScreen } from "../features/startup/StartupStatusScreen";
import { ClosingGuard } from "../features/shutdown/ClosingGuard";
import { InfoEntry } from "./InfoEntry";

/**
 * Selects the GUI entry view from capabilities injected by the native host.
 *
 * Every view sits under the window bar that the page draws where the host hides its native
 * title bar (macOS). Elsewhere the bar renders nothing.
 */
export function ApplicationEntry(props: EntryProps) {
  const customTitlebar = useMemo(() => hasCustomTitlebar(), []);
  return (
    <TitleBarProvider>
      <TitleBar
        icon={diskUnfoldIcon}
        enabled={customTitlebar}
        onDragStart={requestWindowDrag}
        onZoom={requestWindowZoom}
      />
      <EntryView {...props} />
    </TitleBarProvider>
  );
}

interface EntryProps {
  client: AnalysisClient;
  localeEnabled: boolean;
  startupStatusEnabled: boolean;
  analysisEnabled: boolean;
}

function EntryView({
  client,
  localeEnabled,
  startupStatusEnabled,
  analysisEnabled,
}: EntryProps) {
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
