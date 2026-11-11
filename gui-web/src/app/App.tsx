import diskUnfoldIcon from "./assets/diskunfold-icon.svg";
import { useEffect, useMemo, useRef, useState } from "react";
import { Action, BrandMark, Heading, LiveStatus, Text } from "../components";
import { AnalysisPanel } from "../features/analysis/AnalysisPanel";
import { FileInspection } from "../features/analysis/FileInspection";
import { useDisplayPreferences } from "../features/analysis/use-display-preferences";
import { BrowsePanel } from "../features/browse/BrowsePanel";
import { HistoryPanel } from "../features/history/HistoryPanel";
import { SettingsRestoration } from "../features/settings/SettingsRestoration";
import { createAnalysisClient, hasWindowsAnalysisBridge } from "../ipc";
import type { AnalysisClient } from "../ipc";
import { createBrowsePort } from "./browse-port";

const desktop = createAnalysisClient();

export function App({ analysis }: { analysis?: AnalysisClient } = {}) {
  const client = analysis ?? (hasWindowsAnalysisBridge() ? desktop : undefined);
  const preferences = useDisplayPreferences(client);
  const [connectionChecks, setConnectionChecks] = useState(0);
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
      data-browse-open={Boolean(browseVersion) || undefined}
      data-analysis-open={Boolean(analysisOpen) || undefined}
    >
      <header className="du-app-header">
        <div className="du-brand">
          <BrandMark src={diskUnfoldIcon} />
          <Heading level={1}>DiskUnfold</Heading>
        </div>
        <div className="du-app-header__actions">
          <BrowseReturn
            visible={Boolean(browseVersion)}
            onPress={() => {
              restoreBrowseFocus.current = true;
              setBrowseVersion(undefined);
            }}
          />
          {client && (
            <SettingsRestoration
              client={client}
              visible={Boolean(analysisOpen)}
            />
          )}
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
      {!client && (
        <div className="du-analysis-slot">
          <Heading level={2}>Desktop connection unavailable</Heading>
          <Text>
            DiskUnfold cannot connect to its desktop host. Analysis is
            unavailable until the connection is ready.
          </Text>
          <Text>
            Open DiskUnfold from the desktop application. If it is already open,
            close and reopen it, then check the connection again.
          </Text>
          <Action onPress={() => setConnectionChecks((count) => count + 1)}>
            Check connection again
          </Action>
          {connectionChecks > 0 && (
            <LiveStatus>
              Desktop connection is still unavailable. Checks completed:{" "}
              {connectionChecks}.
            </LiveStatus>
          )}
        </div>
      )}
      {client && (
        <div className="du-analysis-slot" hidden={!analysisOpen}>
          <AnalysisPanel
            client={client}
            preferences={preferences}
            onBrowse={setBrowseVersion}
            browseTrigger={browseTrigger}
          />
        </div>
      )}
      {client && browsePort && browseVersion && (
        <div className="du-browse-shell">
          <BrowsePanel
            port={browsePort}
            resultVersion={browseVersion}
            inspectItem={(item, kind) => (
              <FileInspection
                key={`${item.resultVersion}:${item.itemNo}`}
                client={client}
                item={item}
                itemKind={kind}
                display={preferences.display}
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

function BrowseReturn({
  visible,
  onPress,
}: {
  visible: boolean;
  onPress: () => void;
}) {
  if (!visible) return null;
  return (
    <Action variant="quiet" icon="back" autoFocus onPress={onPress}>
      Back to analysis
    </Action>
  );
}
