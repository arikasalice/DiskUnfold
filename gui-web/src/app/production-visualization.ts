import type { BrowseVisualizationProvider } from "./browse-visualization";
import type { VisualizationViewRequest } from "../viz";

/** The one format the first product view asks for. */
const FORMAT = "treemap";
const MEASURE = "size";
/** The one color criterion served until the legend rule table exists. */
const COLOR_CRITERION = "none";
const DENOMINATOR_BASIS = "scope";
/** The registered unit coordinate range; the view window covers all of it. */
const COORDINATE_MAX = 1_000_000;
/** Provisional until the display policy supplies it. */
const MIN_AREA_PX2 = 16;
/** One level of children: names of deeper shapes are not available yet. */
const DEPTH_LIMIT = 1;

/**
 * The product assembly of the Browse space view.
 *
 * Offers the view only for a pinned folder whose catalog serves what is asked:
 * the treemap format, the size measure, the scope denominator and the "none"
 * color criterion. Every request carries the result version of the selection, so
 * the engine never has to pick one. The request leaves out `strategy`: the
 * engine pins the highest registered revision and says which it used in the
 * model. Its viewport is a placeholder that the view replaces with the measured
 * one. Text comes from the view, in the active language and the size display
 * chosen when the folder was opened.
 *
 * It is module-level so the Browse hook sees one stable provider.
 */
export const productionVisualizationProvider: BrowseVisualizationProvider = (
  selection,
  catalog,
) => {
  if (
    selection.location.resultVersion !== selection.resultVersion ||
    !catalog.formats.includes(FORMAT) ||
    !catalog.measures.includes(MEASURE) ||
    !catalog.colorCriteria.includes(COLOR_CRITERION) ||
    !catalog.denominatorCriteria.includes(DENOMINATOR_BASIS)
  )
    return null;
  const request: VisualizationViewRequest = {
    resultRevision: selection.resultVersion,
    format: FORMAT,
    aggregateScopeRef: selection.location,
    navigationScopeRef: selection.location,
    measure: MEASURE,
    sizeBasis: selection.sizeBasis,
    classificationCriterion: "none",
    colorCriterion: COLOR_CRITERION,
    denominatorBasis: DENOMINATOR_BASIS,
    viewport: { width: 1, height: 1, scale: 1 },
    visibleWindow: {
      left: 0,
      top: 0,
      right: COORDINATE_MAX,
      bottom: COORDINATE_MAX,
    },
    minAreaPx2: MIN_AREA_PX2,
    depthLimit: DEPTH_LIMIT,
  };
  // A renderer that cannot draw a model shows the same model's rows instead.
  return { request, canDisplay: () => true, autoLoadRows: true };
};
