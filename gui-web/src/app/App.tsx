import type { SizeDisplay } from "../i18n/bytes";
import { useLocaleText } from "../i18n/presentation";
import { useRegionalFormat } from "../i18n/regional-format";
import type { RegionalFormat } from "../i18n/regional-format";
import type { SizeBasisFilter } from "../features/analysis/space-format";
import diskUnfoldIcon from "./assets/diskunfold-icon.svg";
import "./browse-visualization.css";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  Action,
  BrandMark,
  Heading,
  LiveStatus,
  Notice,
  Text,
} from "../components";
import { AnalysisPanel } from "../features/analysis/AnalysisPanel";
import { FileInspection } from "../features/analysis/FileInspection";
import { useDisplayPreferences } from "../features/analysis/use-display-preferences";
import type { LocaleText } from "../i18n/presentation";
import { BrowsePanel } from "../features/browse/BrowsePanel";
import type {
  BrowseOpenLocation,
  BrowsePort,
} from "../features/browse/BrowsePanel";
import { HistoryPanel } from "../features/history/HistoryPanel";
import { SettingsRestoration } from "../features/settings/SettingsRestoration";
import { LocaleSettings } from "../features/settings/LocaleSettings";
import type { LocaleSettingsProps } from "../features/settings/LocaleSettings";
import { createAnalysisClient, hasWindowsAnalysisBridge } from "../ipc";
import type { AnalysisClient } from "../ipc";
import { createBrowsePort } from "./browse-port";
import { InfoEntry } from "./InfoEntry";
import {
  BrowseVisualizationActions,
  BrowseVisualizationView,
  browseSelectionFromOpen,
  useBrowseVisualization,
} from "./browse-visualization";
import type { BrowseVisualizationProvider } from "./browse-visualization";

const desktop = createAnalysisClient();

export function App({
  analysis,
  visualizationProvider,
  locale,
  nativeLocaleHost = false,
  infoClient,
}: {
  analysis?: AnalysisClient;
  visualizationProvider?: BrowseVisualizationProvider;
  locale?: LocaleSettingsProps;
  nativeLocaleHost?: boolean;
  /** Host connection for the product and support screens when analysis is off. */
  infoClient?: AnalysisClient;
} = {}) {
  const client = analysis ?? (hasWindowsAnalysisBridge() ? desktop : undefined);
  const savedPreferences = useDisplayPreferences(client);
  const regional = useRegionalFormat();
  const text = useLocaleText();
  const preferences = {
    ...savedPreferences,
    display: withRegionalFormat(savedPreferences.display, regional),
  };
  const [connectionChecks, setConnectionChecks] = useState(0);
  const [history, setHistory] = useState(false);
  const [browseSelection, setBrowseSelection] = useState<{
    version: string;
    choice: SizeDisplay | undefined;
  }>();
  const [basisSelection, setBasisSelection] = useState<{
    version: string;
    basis: SizeBasisFilter;
  }>();
  const [visualizationOpen, setVisualizationOpen] = useState(false);
  const { version: browseVersion, choice: browseChoice } =
    browseState(browseSelection);
  const browseTrigger = useRef<HTMLButtonElement>(null);
  const restoreBrowseFocus = useRef(false);
  const visualizationTrigger = useRef<HTMLButtonElement>(null);
  const restoreVisualizationFocus = useRef(false);
  const visualization = useBrowseVisualization(
    client,
    visualizationProvider,
    browseVersion,
  );
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
  useLayoutEffect(() => {
    if (!visualizationOpen && restoreVisualizationFocus.current) {
      visualizationTrigger.current?.focus();
      restoreVisualizationFocus.current = false;
    }
  }, [visualizationOpen]);
  const analysisOpen = client && !history && browseVersion === undefined;
  const chosenDisplay = effectiveDisplay(browseChoice, preferences.display);
  const browseDisplay = withRegionalFormat(chosenDisplay, regional);
  const onSizeBasisChange = useCallback(
    (version: string, basis: SizeBasisFilter) =>
      setBasisSelection({ version, basis }),
    [],
  );
  return (
    <main
      className="du-app"
      data-browse-open={Boolean(browseVersion) || undefined}
      data-analysis-open={Boolean(analysisOpen) || undefined}
      data-visualization-open={visualizationOpen || undefined}
    >
      <header className="du-app-header">
        <div className="du-brand">
          <BrandMark src={diskUnfoldIcon} />
          <Heading level={1}>DiskUnfold</Heading>
        </div>
        <div className="du-app-header__actions">
          <LocaleControls locale={locale} />
          <InfoEntry client={client} fallbackClient={infoClient} />
          <BrowseReturn
            visible={Boolean(browseVersion) && !visualizationOpen}
            autoFocus={!restoreVisualizationFocus.current}
            onPress={() => {
              restoreBrowseFocus.current = true;
              visualization.invalidate();
              setBrowseSelection(undefined);
              setVisualizationOpen(false);
            }}
          />
          <BrowseVisualizationActions
            visible={Boolean(browseVersion)}
            open={visualizationOpen}
            available={Boolean(visualization.assembly)}
            trigger={visualizationTrigger}
            onOpen={() => setVisualizationOpen(true)}
            onClose={() => {
              restoreVisualizationFocus.current = true;
              setVisualizationOpen(false);
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
              {text("ui.app.history", "Operation history")}
            </Action>
          )}
        </div>
      </header>
      <LocaleLoadNotice locale={locale} />
      {analysisUnavailableNotice(nativeLocaleHost, text)}
      {shouldShowConnectionUnavailable(client, nativeLocaleHost) && (
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
            onSizeBasisChange={onSizeBasisChange}
            onBrowse={(version, choice) => {
              visualization.invalidate();
              setBrowseSelection({ version, choice });
            }}
            browseTrigger={browseTrigger}
          />
        </div>
      )}
      <BrowseSurface
        client={client}
        port={browsePort}
        resultVersion={browseVersion}
        display={browseDisplay}
        hidden={visualizationOpen}
        onOpening={visualization.invalidate}
        onOpened={(version, opened) =>
          visualization.opened(
            basisSelection?.version === version
              ? browseSelectionFromOpen(
                  version,
                  opened,
                  browseDisplay,
                  basisSelection.basis,
                )
              : undefined,
          )
        }
      />
      <BrowseVisualizationView
        client={client}
        open={visualizationOpen}
        selection={visualization.selection}
        assembly={visualization.assembly}
      />
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

function shouldShowConnectionUnavailable(
  client: AnalysisClient | undefined,
  nativeLocaleHost: boolean,
) {
  return client === undefined && !nativeLocaleHost;
}

function analysisUnavailableNotice(
  nativeLocaleHost: boolean,
  text: LocaleText,
) {
  if (!nativeLocaleHost) return null;
  return (
    <div className="du-analysis-slot">
      <Notice tone="info">
        {text(
          "ui.analysis.platform_unavailable",
          "Disk analysis is not enabled in this Linux build.",
        )}
      </Notice>
    </div>
  );
}

function withRegionalFormat(
  display: SizeDisplay,
  regional: RegionalFormat | undefined,
): SizeDisplay {
  return regional ? { ...display, regional } : display;
}

function LocaleControls({
  locale,
}: {
  locale: LocaleSettingsProps | undefined;
}) {
  return locale ? <LocaleSettings {...locale} /> : null;
}

function LocaleLoadNotice({
  locale,
}: {
  locale: LocaleSettingsProps | undefined;
}) {
  return locale?.errorKey === "ui.locale.failed" ? (
    <LiveStatus>{locale.text(locale.errorKey)}</LiveStatus>
  ) : null;
}

function BrowseReturn({
  visible,
  autoFocus = true,
  onPress,
}: {
  visible: boolean;
  autoFocus?: boolean;
  onPress: () => void;
}) {
  const text = useLocaleText();
  if (!visible) return null;
  return (
    <Action variant="quiet" icon="back" autoFocus={autoFocus} onPress={onPress}>
      {text("ui.app.back_to_analysis", "Back to analysis")}
    </Action>
  );
}

function BrowseSurface({
  client,
  port,
  resultVersion,
  display,
  hidden,
  onOpening,
  onOpened,
}: {
  client: AnalysisClient | undefined;
  port: BrowsePort | undefined;
  resultVersion: string | undefined;
  display: SizeDisplay;
  hidden: boolean;
  onOpening: () => void;
  onOpened: (resultVersion: string, opened: BrowseOpenLocation) => void;
}) {
  if (!client || !port || !resultVersion) return null;
  return (
    <div
      className="du-browse-shell"
      hidden={hidden}
      style={hidden ? { display: "none" } : undefined}
    >
      <BrowsePanel
        port={port}
        resultVersion={resultVersion}
        display={display}
        onOpening={onOpening}
        onOpened={(opened) => onOpened(resultVersion, opened)}
        inspectItem={(item, kind) => (
          <FileInspection
            key={`${item.resultVersion}:${item.itemNo}`}
            client={client}
            item={item}
            itemKind={kind}
            display={display}
          />
        )}
      />
    </div>
  );
}

function effectiveDisplay(
  choice: SizeDisplay | undefined,
  fallback: SizeDisplay,
): SizeDisplay {
  return choice ?? fallback;
}

function browseState(
  selection: { version: string; choice: SizeDisplay | undefined } | undefined,
) {
  return selection ?? { version: undefined, choice: undefined };
}
