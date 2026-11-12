import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { Ref } from "react";
import { Action, Progress, Text } from "../components";
import type { SizeDisplay } from "../i18n/bytes";
import type { SizeBasisFilter } from "../features/analysis/space-format";
import type { AnalysisClient, ItemRef, VizCatalog } from "../ipc";
import type { BrowseOpenLocation } from "../features/browse/BrowsePanel";
import { VisualizationSession } from "../features/visualization/VisualizationSession";
import type { VisualizationViewRequest } from "../features/visualization/VisualizationSession";
import type { ModelResponse } from "../features/visualization/model-adapter";
import type { VizText } from "../features/visualization/model-text";
import { useLocaleText } from "../i18n/presentation";

export type BrowseVisualizationSelection = {
  resultVersion: string;
  location: ItemRef;
  display: SizeDisplay;
  sizeBasis: SizeBasisFilter;
};

/** App-only assembly supplied by the product owner after catalog inspection. */
export type BrowseVisualizationAssembly = {
  request: VisualizationViewRequest;
  canDisplay: (frame: ModelResponse) => boolean;
  text: VizText;
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
  }>();
  const generation = useRef(0);
  const invalidate = useCallback(() => {
    generation.current += 1;
    setSelection(undefined);
    setState(undefined);
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
        const assembly = provider(selection, catalog);
        if (
          !assembly ||
          !supportsBrowseVisualization(assembly, catalog, selection)
        )
          return;
        setState({ key, provider, assembly });
      })
      .catch(() => {
        if (active && requestGeneration === generation.current)
          setState(undefined);
      });
    return () => {
      active = false;
    };
  }, [client, provider, resultVersion, selection]);

  const key = selection ? JSON.stringify(selection) : undefined;
  const assembly =
    key && state?.key === key && state.provider === provider
      ? state.assembly
      : undefined;
  return {
    selection: assembly ? selection : undefined,
    assembly,
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
  return {
    resultVersion: page.resultVersion,
    location: opened.location,
    display,
    sizeBasis,
  };
}

export function BrowseVisualizationActions({
  visible,
  open,
  available,
  trigger,
  onOpen,
  onClose,
}: {
  visible: boolean;
  open: boolean;
  available: boolean;
  trigger: Ref<HTMLButtonElement>;
  onOpen: () => void;
  onClose: () => void;
}) {
  const text = useLocaleText();
  return (
    <>
      {visible && open && (
        <Action variant="quiet" icon="back" autoFocus onPress={onClose}>
          {text("ui.visualization.back_to_folders", "Back to folders")}
        </Action>
      )}
      {visible && available && (
        <div hidden={open}>
          <Action variant="quiet" icon="layers" ref={trigger} onPress={onOpen}>
            {text("ui.visualization.show_space_distribution", "Show space distribution")}
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
}: {
  client: AnalysisClient | undefined;
  open: boolean;
  selection: BrowseVisualizationSelection | undefined;
  assembly: BrowseVisualizationAssembly | undefined;
}) {
  const text = useLocaleText();
  const viewportElement = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState<
    VisualizationViewRequest["viewport"] | undefined
  >();

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
      const rect = element.getBoundingClientRect();
      update(rect.width, rect.height);
    };
    const observer = new ResizeObserver((entries) => {
      const entry = entries.find(({ target }) => target === element);
      if (entry) update(entry.contentRect.width, entry.contentRect.height);
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
        <Text>{text("ui.visualization.unavailable_for_folder", "Space distribution is unavailable for this folder.")}</Text>
      </div>
    );
  return (
    <section className="du-visualization-shell" aria-label={text("ui.visualization.space_distribution", "Space distribution")}>
      <div ref={viewportElement} className="du-visualization-viewport">
        {viewport ? (
          <VisualizationSession
            client={client}
            request={{ ...assembly.request, viewport }}
            canDisplay={(frame) =>
              frameMatchesBrowseSelection(frame, selection) &&
              assembly.canDisplay(frame)
            }
            text={assembly.text}
            autoLoadRows={assembly.autoLoadRows}
            {...(assembly.resolveRenderer
              ? { resolveRenderer: assembly.resolveRenderer }
              : {})}
          />
        ) : (
          <Progress label={text("ui.visualization.loading", "Loading visualization")} />
        )}
      </div>
    </section>
  );
}

function supportsBrowseVisualization(
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
    strategy !== undefined &&
    catalog.strategies.some(
      (candidate) =>
        candidate.id === strategy.id &&
        candidate.revision === strategy.revision,
    )
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
