import type { SizeDisplay } from "../i18n/bytes";
import { useLocaleText } from "../i18n/presentation";
import { useRegionalFormat } from "../i18n/regional-format";
import type { RegionalFormat } from "../i18n/regional-format";
import type { SizeBasisFilter } from "../features/analysis/space-format";
import diskUnfoldIcon from "./assets/diskunfold-icon.svg";
import "./browse-visualization.css";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Action,
  BrandMark,
  Heading,
  LiveStatus,
  Notice,
  Text,
  useTitleBarContext,
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
import { GroupsEntryAction } from "../features/groups/GroupsEntry";
import { GroupsSurface } from "../features/groups/GroupsSurface";
import { useGroupsEntry } from "../features/groups/use-groups-entry";
import { HistoryPanel } from "../features/history/HistoryPanel";
import { HistorySizeDisplay } from "../features/history/history-size";
import { JobCenterEntry } from "../features/jobs/JobCenterEntry";
import { useJobCenterParts } from "../features/jobs/JobCenter";
import { SettingsRestoration } from "../features/settings/SettingsRestoration";
import { LocaleSettings } from "../features/settings/LocaleSettings";
import type { LocaleSettingsProps } from "../features/settings/LocaleSettings";
import { createAnalysisClient, hasDesktopAnalysisBridge } from "../ipc";
import type { AnalysisClient } from "../ipc";
import {
  type ScanPriority,
  useScanPriority,
  useScanPriorityContext,
} from "../state/scan-priority";
import {
  browseOpener,
  priorityView,
  useBrowseSelection,
} from "./browse-selection";
import { createBrowsePort } from "./browse-port";
import { usePageScroll } from "./page-scroll";
import { InfoEntry } from "./InfoEntry";
import { SampleContext, SampleEntrySlot } from "./SampleBrowse";
import { useSampleFlow } from "./use-sample-flow";
import {
  BrowseVisualizationActions,
  BrowseVisualizationView,
  useBrowseVisualization,
  useVisualizationFocus,
} from "./browse-visualization";
import type { BrowseVisualizationProvider } from "./browse-visualization";
import { useVisualizationOffer } from "./browse-visualization-offer";

const desktop = createAnalysisClient();

function analysisVisible(screen: {
  connected: boolean;
  history: boolean;
  browsing: boolean;
  sample: boolean;
}) {
  return (
    screen.connected && !screen.history && !screen.browsing && !screen.sample
  );
}

/** Screen name for the window title bar, or nothing while there is no desktop connection. */
function screenLabel(
  text: LocaleText,
  screen: {
    connected: boolean;
    history: boolean;
    browsing: boolean;
    groups: boolean;
    sample: boolean;
  },
) {
  if (!screen.connected) return undefined;
  if (screen.history) return text("ui.app.history", "Operation history");
  if (screen.sample) return text("ui.sample.screen", "Sample experience");
  if (screen.groups) return text("ui.app.screen_groups", "Group by my rules");
  if (screen.browsing) return text("ui.app.screen_browse", "Browse folders");
  return text("ui.app.screen_analysis", "Analysis");
}

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
  const client = analysis ?? (hasDesktopAnalysisBridge() ? desktop : undefined);
  const savedPreferences = useDisplayPreferences(client);
  const regional = useRegionalFormat();
  const text = useLocaleText();
  const preferences = {
    ...savedPreferences,
    display: withRegionalFormat(savedPreferences.display, regional),
  };
  const [connectionChecks, setConnectionChecks] = useState(0);
  const [history, setHistory] = useState(false);
  const [tasksOpen, setTasksOpen] = useState(false);
  const browse = useBrowseSelection();
  const [basisSelection, setBasisSelection] = useState<{
    version: string;
    basis: SizeBasisFilter;
  }>();
  const [visualizationOpen, setVisualizationOpen] = useState(false);
  const [scanContext, setScanContext] = useScanPriorityContext();
  const flow = useSampleFlow(client);
  const { sampleOpen, analysisKey, analysisSlot } = flow;
  const { version: browseVersion, choice: browseChoice } = browse;
  const groups = useGroupsEntry(browseVersion);
  const browseTrigger = useRef<HTMLButtonElement>(null);
  const tasksTrigger = useRef<HTMLButtonElement>(null);
  // Names the screen for the window title bar: operation history, folder browsing or analysis.
  useTitleBarContext(
    screenLabel(text, {
      connected: Boolean(client),
      history,
      browsing: Boolean(browseVersion),
      groups: groups.open,
      sample: sampleOpen,
    }),
  );
  const restoreBrowseFocus = useRef(false);
  // False while the folder list opens, when its title takes the focus; later
  // "Back to analysis" takes it, as after a view over the list closes.
  const browseShown = useRef(false);
  useEffect(() => {
    browseShown.current = Boolean(browseVersion);
  }, [browseVersion]);
  const visualizationTrigger = useRef<HTMLButtonElement>(null);
  const frame = useRef<HTMLElement>(null);
  usePageScroll(frame);
  const visualizationFocus = useVisualizationFocus(
    visualizationOpen,
    setVisualizationOpen,
    visualizationTrigger,
  );
  const visualization = useBrowseVisualization(
    client,
    visualizationProvider,
    browseVersion,
  );
  const openBrowse = browseOpener(visualization.invalidate, browse.set);
  const priority = useScanPriority(
    client,
    scanContext,
    priorityView(browse.selection, openBrowse),
  );
  const browsePort = useMemo(
    () => (client && browseVersion ? createBrowsePort(client) : undefined),
    [client, browseVersion],
  );
  useEffect(() => () => browsePort?.dispose(), [browsePort]);
  const restoreFocus = useRef(false);
  useEffect(() => {
    if (!history && restoreFocus.current) {
      tasksTrigger.current?.focus();
      restoreFocus.current = false;
    }
  }, [history]);
  useEffect(() => {
    if (!browseVersion && restoreBrowseFocus.current) {
      browseTrigger.current?.focus();
      restoreBrowseFocus.current = false;
    }
  }, [browseVersion]);
  const analysisOpen = analysisVisible({
    connected: Boolean(client),
    history,
    browsing: browseVersion !== undefined,
    sample: sampleOpen,
  });
  const controls = browseControls(
    Boolean(browseVersion),
    visualizationOpen,
    groups.open,
  );
  const chosenDisplay = effectiveDisplay(browseChoice, preferences.display);
  const browseDisplay = withRegionalFormat(chosenDisplay, regional);
  const onSizeBasisChange = useCallback(
    (version: string, basis: SizeBasisFilter) =>
      setBasisSelection({ version, basis }),
    [],
  );
  const visualizationOffer = useVisualizationOffer(
    basisSelection,
    browseDisplay,
    visualization.opened,
  );
  const jobCenter = useJobCenterParts({
    client,
    isOpen: tasksOpen,
    trigger: tasksTrigger,
    renderEntry: (count, onPress) => (
      <JobCenterEntry
        clientAvailable={client !== undefined}
        historyOpen={history}
        count={count}
        expanded={tasksOpen}
        trigger={tasksTrigger}
        onPress={onPress}
      />
    ),
    onOpenChange: setTasksOpen,
    onOpenHistory: () => setHistory(true),
    onOpenJob: (card) => {
      if (card.row.kind === "scan.metadata") {
        if (sampleOpen)
          void flow.endSample().then((ended) => {
            if (ended) flow.startRealAnalysis();
          });
        browse.set(undefined);
        setVisualizationOpen(false);
        setHistory(false);
      } else {
        setHistory(true);
      }
    },
  });
  return (
    <main
      ref={frame}
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
            visible={controls.backVisible}
            autoFocus={
              browseShown.current &&
              !visualizationFocus.restoring() &&
              !groups.restoringFocus()
            }
            onPress={() => {
              restoreBrowseFocus.current = true;
              visualization.invalidate();
              browse.set(undefined);
              setVisualizationOpen(false);
            }}
          />
          <BrowseVisualizationActions
            visible={controls.visualizationVisible}
            open={visualizationOpen}
            available={Boolean(visualization.assembly)}
            catalogFailure={visualization.catalogFailure}
            trigger={visualizationTrigger}
            onOpen={() => setVisualizationOpen(true)}
            onClose={visualizationFocus.close}
          />
          <GroupsEntryAction
            // The sample screen browses on its own; there the entry stays disabled with its reason.
            visible={groups.entryVisible(visualizationOpen) || sampleOpen}
            sample={groups.sample || sampleOpen}
            trigger={groups.trigger}
            onOpen={groups.show}
          />
          {client && (
            <SettingsRestoration
              client={client}
              visible={Boolean(analysisOpen)}
            />
          )}
          {jobCenter.entry}
        </div>
      </header>
      <LocaleLoadNotice locale={locale} />
      <SampleContext
        client={client}
        flow={flow}
        display={withRegionalFormat(preferences.display, regional)}
      />
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
        <div
          ref={analysisSlot}
          tabIndex={-1}
          className="du-analysis-slot"
          hidden={!analysisOpen}
        >
          <AnalysisPanel
            key={analysisKey}
            client={client}
            preferences={preferences}
            onSizeBasisChange={onSizeBasisChange}
            onBrowse={(version, choice, scan) =>
              openBrowse({ version, choice, scan })
            }
            browseTrigger={browseTrigger}
            onScanContext={setScanContext}
            priorityArea={priority.requestedArea}
            startAlternative={<SampleEntrySlot flow={flow} />}
          />
        </div>
      )}
      <BrowseSurface
        client={client}
        port={browsePort}
        resultVersion={browseVersion}
        display={browseDisplay}
        hidden={controls.listHidden}
        priority={priority}
        onOpening={() => {
          visualization.invalidate();
          visualizationOffer.opening();
        }}
        onOpened={(version, opened) => {
          groups.onBrowsed(version, opened);
          visualizationOffer.browsed(version, opened);
        }}
      />
      <BrowseVisualizationView
        client={client}
        open={visualizationOpen}
        selection={visualization.selection}
        assembly={visualization.assembly}
        catalog={visualization.catalog}
      />
      <GroupsSurface
        client={client}
        browseVersion={browseVersion}
        entry={groups}
        display={browseDisplay}
      />
      {jobCenter.overlay}
      {client && history && (
        <HistorySizeDisplay.Provider value={browseDisplay}>
          <HistoryPanel
            client={client}
            onClose={() => {
              restoreFocus.current = true;
              setHistory(false);
            }}
          />
        </HistorySizeDisplay.Provider>
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
  priority,
  onOpening,
  onOpened,
}: {
  client: AnalysisClient | undefined;
  port: BrowsePort | undefined;
  resultVersion: string | undefined;
  display: SizeDisplay;
  hidden: boolean;
  priority: ScanPriority;
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
        priority={priority}
        focusTitle
        onOpening={onOpening}
        onOpened={(opened) => onOpened(resultVersion, opened)}
        inspectItem={(item, kind, countedZero) => (
          <FileInspection
            key={`${item.resultVersion}:${item.itemNo}`}
            client={client}
            item={item}
            itemKind={kind}
            display={display}
            countedZero={countedZero}
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

/** Which browse controls show while the space view or the virtual-group view is open. */
function browseControls(
  browsing: boolean,
  visualizationOpen: boolean,
  groupsOpen: boolean,
) {
  const overlayOpen = visualizationOpen || groupsOpen;
  return {
    backVisible: browsing && !overlayOpen,
    visualizationVisible: browsing && !groupsOpen,
    listHidden: overlayOpen,
  };
}
