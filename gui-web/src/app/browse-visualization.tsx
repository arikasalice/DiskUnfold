import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { RefObject } from "react";
import {
  Action,
  Heading,
  Notice,
  Progress,
  SegmentedChoice,
  Text,
} from "../components";
import type { SizeDisplay } from "../i18n/bytes";
import type { SizeBasisFilter } from "../features/analysis/space-format";
import type { AnalysisClient, ItemRef, VizCatalog } from "../ipc";
import type { BrowseOpenLocation } from "../features/browse/BrowsePanel";
import { useLocaleOsFamily } from "../state/use-locale";
import { defaultSizeDisplay } from "../state/size-display";
import { VisualizationSession } from "../features/visualization/VisualizationSession";
import {
  ChartAreaContext,
  CHART_FOCUS_ROOM,
  chartHeight,
  usableSize,
} from "../features/visualization/chart-area";
import {
  useProductionVizText,
  visualizationFormatName,
  vizMessage,
} from "../viz";
import type { ModelResponse, VisualizationViewRequest } from "../viz";
import type { VizText } from "../viz";
import { useLocaleText } from "../i18n/presentation";
import { shownHeight } from "./page-scroll";
import { useHistogramAxis } from "./histogram-axis";
import { useLegendChoice } from "./legend-choice";
import { TypeListPlace, useTypeListView } from "./type-list-place";

export type BrowseVisualizationSelection = {
  resultVersion: string;
  location: ItemRef;
  display: SizeDisplay;
  sizeBasis: SizeBasisFilter;
  /** Display names from the scope root to this folder; absent when any name is unknown. */
  locationTrail?: readonly string[];
};

/** App-only assembly supplied by the product owner after catalog inspection. */
export type BrowseVisualizationAssembly = {
  request: VisualizationViewRequest;
  canDisplay: (frame: ModelResponse) => boolean;
  /** Omitted: the view builds the product text in the active language. */
  text?: VizText;
  autoLoadRows: boolean;
  resolveRenderer?: Parameters<
    typeof VisualizationSession
  >[0]["resolveRenderer"];
};

/** Must be stable and side-effect free; called after a pinned Browse location and catalog exist. */
export type BrowseVisualizationProvider = (
  selection: BrowseVisualizationSelection,
  catalog: VizCatalog,
) => BrowseVisualizationAssembly | null;

/** Where the chart is assumed to start in the scroll area until it reports its place. */
const DEFAULT_CHART_OFFSET = 40;

/**
 * A `viz.catalog` request that failed for the current folder, kept apart from
 * an empty catalog: only a failure offers a retry. It is never retried on its own.
 */
export type BrowseCatalogFailure = { retrying: boolean; retry: () => void };

export function useBrowseVisualization(
  client: AnalysisClient | undefined,
  provider: BrowseVisualizationProvider | undefined,
  resultVersion: string | undefined,
) {
  const [selection, setSelection] = useState<BrowseVisualizationSelection>();
  const [state, setState] = useState<{
    key: string;
    provider: BrowseVisualizationProvider;
    assembly: BrowseVisualizationAssembly;
    catalog: VizCatalog;
  }>();
  const [failure, setFailure] = useState<{ key: string; retrying: boolean }>();
  const [attempt, setAttempt] = useState(0);
  const generation = useRef(0);
  const invalidate = useCallback(() => {
    generation.current += 1;
    setSelection(undefined);
    setState(undefined);
    setFailure(undefined);
  }, []);
  const opened = useCallback(
    (next: BrowseVisualizationSelection | undefined) => {
      setSelection(next);
    },
    [],
  );

  useEffect(() => {
    if (
      !client ||
      !provider ||
      !selection ||
      selection.resultVersion !== resultVersion
    ) {
      setState(undefined);
      return;
    }
    let active = true;
    const requestGeneration = generation.current;
    const key = JSON.stringify(selection);
    setState(undefined);
    void client
      .call("viz.catalog", {})
      .then((catalog) => {
        if (!active || requestGeneration !== generation.current) return;
        setFailure(undefined);
        const assembly = provider(selection, catalog);
        if (
          !assembly ||
          !supportsBrowseVisualization(assembly, catalog, selection)
        )
          return;
        setState({ key, provider, assembly, catalog });
      })
      .catch(() => {
        if (active && requestGeneration === generation.current) {
          setState(undefined);
          setFailure({ key, retrying: false });
        }
      });
    return () => {
      active = false;
    };
  }, [client, provider, resultVersion, selection, attempt]);

  const key = selection ? JSON.stringify(selection) : undefined;
  const assembly =
    key && state?.key === key && state.provider === provider
      ? state.assembly
      : undefined;
  const catalog = assembly ? state?.catalog : undefined;
  const retry = useCallback(() => {
    setFailure((current) => current && { ...current, retrying: true });
    setAttempt((current) => current + 1);
  }, []);
  const catalogFailure: BrowseCatalogFailure | undefined =
    key && failure?.key === key
      ? { retrying: failure.retrying, retry }
      : undefined;
  return {
    selection: assembly ? selection : undefined,
    assembly,
    catalog,
    catalogFailure,
    invalidate,
    opened,
  };
}

export function browseSelectionFromOpen(
  expectedVersion: string,
  opened: BrowseOpenLocation,
  display: SizeDisplay,
  sizeBasis: SizeBasisFilter,
): BrowseVisualizationSelection | undefined {
  const { page } = opened;
  if (
    page.resultVersion !== expectedVersion ||
    opened.rootKind === "virtual_root" ||
    !opened.location ||
    opened.location.resultVersion !== page.resultVersion
  )
    return undefined;
  const locationTrail = trailOf(opened);
  return {
    resultVersion: page.resultVersion,
    location: opened.location,
    display,
    sizeBasis,
    ...(locationTrail ? { locationTrail } : {}),
  };
}

/** The folder's display names, or undefined when they do not match the opened location. */
function trailOf(opened: BrowseOpenLocation): string[] | undefined {
  const { location, breadcrumbs, page } = opened;
  const current = breadcrumbs.at(-1);
  if (
    !location ||
    !current ||
    location.resultVersion !== page.resultVersion ||
    !sameItemRef(current.item, location)
  )
    return undefined;
  const names = breadcrumbs.map(({ displayName }) =>
    displayName.state === "present" ? displayName.value : null,
  );
  return names.every((name) => name !== null) ? (names as string[]) : undefined;
}

/**
 * Closing the space view returns the focus to its open button. The returned
 * `close` is the one handler for every control that leaves the view.
 */
export function useVisualizationFocus(
  open: boolean,
  setOpen: (open: boolean) => void,
  trigger: RefObject<HTMLButtonElement | null>,
) {
  const restore = useRef(false);
  useLayoutEffect(() => {
    if (!open && restore.current) {
      trigger.current?.focus();
      restore.current = false;
    }
  }, [open, trigger]);
  const close = useCallback(() => {
    restore.current = true;
    setOpen(false);
  }, [setOpen]);
  return { close, restoring: () => restore.current };
}

export function BrowseVisualizationActions({
  visible,
  open,
  available,
  catalogFailure,
  trigger,
  onOpen,
  onClose,
}: {
  visible: boolean;
  open: boolean;
  available: boolean;
  catalogFailure?: BrowseCatalogFailure | undefined;
  trigger: RefObject<HTMLButtonElement | null>;
  onOpen: () => void;
  onClose: () => void;
}) {
  const text = useLocaleText();
  // A retry that succeeds moves the focus to the button it brought back.
  const retried = useRef(false);
  useLayoutEffect(() => {
    if (available && retried.current) trigger.current?.focus();
    if (available || !catalogFailure) retried.current = false;
  }, [available, catalogFailure, trigger]);
  return (
    <>
      {visible && open && (
        <Action variant="quiet" icon="back" onPress={onClose}>
          {text("ui.visualization.back_to_folders", "Back to folders")}
        </Action>
      )}
      {visible && !open && !available && catalogFailure && (
        <Notice tone="error">
          {text(
            "ui.visualization.catalog_failed",
            "The space distribution could not be prepared.",
          )}{" "}
          <Action
            isPending={catalogFailure.retrying}
            onPress={() => {
              retried.current = true;
              catalogFailure.retry();
            }}
          >
            {catalogFailure.retrying
              ? text("ui.browse.retrying", "Retrying…")
              : text("ui.browse.retry", "Retry")}
          </Action>
        </Notice>
      )}
      {visible && available && (
        <div hidden={open}>
          <Action icon="layers" ref={trigger} onPress={onOpen}>
            {text(
              "ui.visualization.show_space_distribution",
              "Show space distribution",
            )}
          </Action>
        </div>
      )}
    </>
  );
}

export function BrowseVisualizationView({
  client,
  open,
  selection,
  assembly,
  catalog,
}: {
  client: AnalysisClient | undefined;
  open: boolean;
  selection: BrowseVisualizationSelection | undefined;
  assembly: BrowseVisualizationAssembly | undefined;
  /** The formats offered in the format strip, in catalog order (F06 README 3). */
  catalog?: VizCatalog | undefined;
}) {
  const text = useLocaleText();
  const osFamily = useLocaleOsFamily();
  const productionText = useProductionVizText(
    selection?.display ?? defaultSizeDisplay(osFamily),
  );
  const vizText = assembly?.text ?? productionText;
  const viewportElement = useRef<HTMLDivElement>(null);
  // The screen opens with the focus on its title, without a frame; one Tab
  // reaches the chart (README 21, 34).
  const title = useRef<HTMLHeadingElement>(null);
  const titleFocused = useRef(false);
  const ready = open && !!client && !!assembly && !!selection;
  useLayoutEffect(() => {
    if (!open) titleFocused.current = false;
    else if (ready && !titleFocused.current) {
      titleFocused.current = true;
      title.current?.focus({ preventScroll: true });
    }
  }, [open, ready]);
  const [viewport, setViewport] = useState<
    VisualizationViewRequest["viewport"] | undefined
  >();
  const [chartParts, setChartParts] = useState({
    offset: DEFAULT_CHART_OFFSET,
  });
  const formats = useFormatChoice(
    open ? client : undefined,
    catalog,
    assembly,
    selection,
  );
  const axis = useHistogramAxis(formats.format, catalog, assembly, vizText);
  const legend = useLegendChoice(formats.format, catalog, assembly, vizText);
  // "Show in list" puts the file list in the chart's place (README 46.13).
  const list = useTypeListView(open, client, legend.narrowing);
  // A new format starts at the top of its chart, not where the last one was scrolled.
  useLayoutEffect(() => {
    viewportElement.current?.scrollTo?.({ top: 0 });
  }, [formats.format]);
  const area = useMemo(
    () => ({
      element: viewportElement,
      reportOffset: (offset: number) =>
        setChartParts((current) =>
          Math.abs(current.offset - offset) < 1
            ? current
            : { offset: Math.round(offset) },
        ),
    }),
    [],
  );

  useLayoutEffect(() => {
    if (!open || !client || !assembly || !selection) {
      setViewport(undefined);
      return;
    }
    const element = viewportElement.current;
    if (!element || typeof ResizeObserver === "undefined") {
      setViewport(undefined);
      return;
    }
    let active = true;
    const update = (width: number, height: number) => {
      if (!active) return;
      const scale = window.devicePixelRatio;
      const logicalWidth = Math.round(width);
      const logicalHeight = Math.round(height);
      if (
        !Number.isFinite(logicalWidth) ||
        !Number.isFinite(logicalHeight) ||
        logicalWidth <= 0 ||
        logicalHeight <= 0 ||
        logicalWidth > 0xffff_ffff ||
        logicalHeight > 0xffff_ffff ||
        !Number.isFinite(scale) ||
        scale <= 0
      ) {
        setViewport(undefined);
        return;
      }
      setViewport((current) =>
        current?.width === logicalWidth &&
        current.height === logicalHeight &&
        current.scale === scale
          ? current
          : { width: logicalWidth, height: logicalHeight, scale },
      );
    };
    const measure = () => {
      const { width, height } = usableSize(
        element.getBoundingClientRect(),
        element,
      );
      update(width - 2 * CHART_FOCUS_ROOM, shownHeight(height));
    };
    const observer = new ResizeObserver((entries) => {
      const entry = entries.find(({ target }) => target === element);
      if (entry)
        update(
          entry.contentRect.width - 2 * CHART_FOCUS_ROOM,
          shownHeight(entry.contentRect.height),
        );
    });
    observer.observe(element);
    measure();
    window.addEventListener("resize", measure);
    window.visualViewport?.addEventListener("resize", measure);
    return () => {
      active = false;
      observer.disconnect();
      window.removeEventListener("resize", measure);
      window.visualViewport?.removeEventListener("resize", measure);
    };
  }, [
    assembly,
    client,
    open,
    selection?.location.itemNo,
    selection?.resultVersion,
  ]);

  if (!open) return null;
  if (!client || !assembly || !selection)
    return (
      <div className="du-visualization-shell">
        <Text>
          {text(
            "ui.visualization.unavailable_for_folder",
            "Space distribution is unavailable for this folder.",
          )}
        </Text>
      </div>
    );
  return (
    <section
      className="du-visualization-shell"
      aria-label={text(
        "ui.visualization.space_distribution",
        "Space distribution",
      )}
    >
      <header className="du-visualization-title">
        <Heading level={2} ref={title} programmaticFocus>
          {text("ui.visualization.space_distribution", "Space distribution")}
        </Heading>
        {selection.locationTrail && (
          <Text kind="path">{selection.locationTrail.join(" › ")}</Text>
        )}
        {formats.options.length > 1 && (
          <div ref={formats.strip} hidden={list.listing}>
            <SegmentedChoice
              label={vizText.message("formatChoice")}
              value={formats.format}
              onChange={formats.choose}
              options={formats.options.map((format) => ({
                value: format,
                label: visualizationFormatName(format, vizText.message),
                note: formats.noData(format)
                  ? vizText.message("formatNoData")
                  : undefined,
              }))}
            />
          </div>
        )}
      </header>
      <TypeListPlace
        port={list.port}
        resultVersion={selection.resultVersion}
        scope={selection.location}
        narrowing={legend.narrowing}
        text={vizText}
        title={title}
      />
      <div
        ref={viewportElement}
        className="du-visualization-viewport"
        hidden={list.listing}
      >
        {viewport ? (
          <ChartAreaContext.Provider value={area}>
            <VisualizationSession
              client={client}
              request={{
                ...assembly.request,
                format: formats.format,
                depthLimit: formats.depthLimit,
                measure: axis.measure,
                ...legend.request,
                viewport: {
                  ...viewport,
                  width: legend.chartWidth(viewport.width),
                  height: chartHeight(viewport.height, chartParts.offset),
                },
              }}
              canDisplay={(frame) =>
                frameMatchesBrowseSelection(frame, selection) &&
                assembly.canDisplay(frame)
              }
              text={vizText}
              autoLoadRows={assembly.autoLoadRows}
              onAvailableFormats={formats.learn}
              statusAction={axis.control}
              legend={legend.controls}
              narrowing={legend.narrowing}
              legendSide={legend.side(viewport.width)}
              {...(formats.available
                ? {
                    formatAction: {
                      label: viewAsLabel(formats.available, vizText),
                      onPress: () => {
                        if (formats.available)
                          formats.choose(formats.available, true);
                      },
                    },
                  }
                : {})}
              {...(assembly.resolveRenderer
                ? { resolveRenderer: assembly.resolveRenderer }
                : {})}
            />
          </ChartAreaContext.Provider>
        ) : (
          <Progress
            label={text("ui.visualization.loading", "Loading visualization")}
          />
        )}
      </div>
    </section>
  );
}

/**
 * Whether the owner's request fits the pinned Browse selection and the catalog.
 *
 * A request that leaves `strategy` out is accepted: the engine then pins the
 * highest registered revision for the format and reports it in the model, so the
 * screen never invents a strategy id. A request that names a strategy must name
 * one the catalog lists.
 */
export function supportsBrowseVisualization(
  assembly: BrowseVisualizationAssembly,
  catalog: VizCatalog,
  selection: BrowseVisualizationSelection,
): boolean {
  const { request } = assembly;
  const { strategy } = request;
  return (
    request.resultRevision === selection.resultVersion &&
    request.sizeBasis === selection.sizeBasis &&
    sameItemRef(request.aggregateScopeRef, selection.location) &&
    sameItemRef(request.navigationScopeRef, selection.location) &&
    (!request.focusItemRef ||
      request.focusItemRef.resultVersion === selection.resultVersion) &&
    (request.expandedDepthByItem ?? []).every(
      ({ itemRef }) => itemRef.resultVersion === selection.resultVersion,
    ) &&
    catalog.formats.includes(request.format) &&
    catalog.measures.includes(request.measure) &&
    catalog.colorCriteria.includes(request.colorCriterion) &&
    catalog.denominatorCriteria.includes(request.denominatorBasis) &&
    (strategy === undefined ||
      catalog.strategies.some(
        (candidate) =>
          candidate.id === strategy.id &&
          candidate.revision === strategy.revision,
      ))
  );
}

function frameMatchesBrowseSelection(
  frame: ModelResponse,
  selection: BrowseVisualizationSelection,
): boolean {
  const { model } = frame;
  return (
    frame.resultRevision === selection.resultVersion &&
    model.representation.sizeBasis === selection.sizeBasis &&
    sameItemRef(model.aggregateScopeRef, selection.location) &&
    sameItemRef(model.navigationScopeRef, selection.location) &&
    frameItemRefs(frame).every(
      (item) => item.resultVersion === selection.resultVersion,
    )
  );
}

function frameItemRefs(frame: ModelResponse): ItemRef[] {
  const refs = [
    frame.model.aggregateScopeRef,
    frame.model.navigationScopeRef,
    ...(frame.model.focusItemRef ? [frame.model.focusItemRef] : []),
  ];
  for (const shape of frame.model.shapes) {
    if (shape.itemRef) refs.push(shape.itemRef);
    if (shape.rootBranchRef) refs.push(shape.rootBranchRef);
  }
  return refs;
}

function sameItemRef(left: ItemRef, right: ItemRef): boolean {
  return (
    left.resultVersion === right.resultVersion && left.itemNo === right.itemNo
  );
}

const viewAsKeys = {
  treemap: "viewAsTreemap",
  bar: "viewAsBar",
  histogram: "viewAsHistogram",
  trend: "viewAsTrend",
  waterfall: "viewAsWaterfall",
  sankey: "viewAsSankey",
} as const;

/** "View as {format}" for the unavailable notice (F06 README 41). */
function viewAsLabel(format: string, text: VizText): string {
  const key = viewAsKeys[format as keyof typeof viewAsKeys];
  return key
    ? text.message(key)
    : vizMessage(text, "viewAsFormat", {
        format: visualizationFormatName(format, text.message),
      });
}

/**
 * The chosen chart format of the space view (F06 README 3, 9).
 *
 * The strip lists the catalog formats in catalog order and starts on the
 * owner's format. A choice is kept while the view moves between folders; the
 * request changes only its format, so the selection, scope, denominator and
 * colour criterion stay. What can be drawn is asked with `viz.availability`
 * when the view opens on a result and folder, and also learned from an
 * unavailable answer, which lists the formats the engine can draw; until one
 * of them arrives no format carries a mark. Choosing a format from the
 * unavailable notice returns the focus to the strip.
 *
 * The owner's depth belongs to the owner's format (a treemap names the shapes
 * inside a folder); every other format asks for one level, which is what the
 * engine accepts for them.
 */
function useFormatChoice(
  client: AnalysisClient | undefined,
  catalog: VizCatalog | undefined,
  assembly: BrowseVisualizationAssembly | undefined,
  selection: BrowseVisualizationSelection | undefined,
) {
  const [chosen, setChosen] = useState<string>();
  const [learned, setLearned] = useState<{
    key: string;
    formats: readonly string[];
  }>();
  const strip = useRef<HTMLDivElement>(null);
  const refocus = useRef(false);
  const key = selection
    ? `${selection.resultVersion}:${selection.location.itemNo}:${selection.sizeBasis}`
    : "";
  const options = catalog?.formats ?? [];
  const fallback = assembly?.request.format ?? "";
  const format = chosen && options.includes(chosen) ? chosen : fallback;
  const depthLimit =
    format === fallback ? (assembly?.request.depthLimit ?? 1) : 1;
  const known = learned?.key === key ? learned.formats : undefined;
  const available =
    known && !known.includes(format)
      ? options.find((option) => known.includes(option))
      : undefined;
  useLayoutEffect(() => {
    if (!refocus.current) return;
    refocus.current = false;
    strip.current?.querySelector<HTMLInputElement>("input:checked")?.focus();
  }, [format]);
  const choose = useCallback((next: string, focusStrip = false) => {
    refocus.current = focusStrip;
    setChosen(next);
  }, []);
  const learn = useCallback(
    (formats: readonly string[]) => setLearned({ key, formats }),
    [key],
  );
  // Asked once per result and folder, so the marks show before a choice (#241).
  // A failed read leaves the marks to the next unavailable answer.
  useEffect(() => {
    if (!client || !selection || options.length < 2) return undefined;
    let active = true;
    void client
      .call("viz.availability", {
        resultRevision: selection.resultVersion,
        aggregateScopeRef: selection.location,
      })
      .then((answer) => {
        if (active)
          learn(
            answer.formats
              .filter((entry) => entry.available)
              .map((entry) => entry.format),
          );
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [client, selection, learn, options.length]);
  return {
    format,
    depthLimit,
    options,
    strip,
    available,
    noData: (option: string) => !!known && !known.includes(option),
    choose,
    learn,
  };
}
