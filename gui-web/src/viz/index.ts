/**
 * Public entry of the visualization model layer. It turns the generated
 * visualization contract into display props and text and holds no screen
 * state. Code outside `src/viz` imports from here only.
 */
export {
  adaptVizModel,
  matchingRowsPage,
  selectionKey,
  visualizationShapeRef,
} from "./model-adapter";
export type {
  ModelResponse,
  ModelRowsPage,
  VizSelection,
} from "./model-adapter";
export { rawSourceSize, shapeLabels, vizMessage } from "./model-text";
export { plainRowName, rowName } from "./row-text";
export { explainReason } from "./reason-text";
export { MAX_ROWS_PER_READ, readRowsPages } from "./rows-pages";
export type { VizText } from "./model-text";
export {
  visualizationFormatName,
  visualizationMessage,
  visualizationMessages,
  visualizationReason,
} from "./messages";
export { visualizationUnavailableText } from "./unavailable-text";
export type {
  VisualizationLanguage,
  VisualizationMessageKey,
} from "./messages";
export { ModelEvidence } from "./ModelEvidence";
export { sameItem } from "./item-ref";
export { VizModelLoader } from "./model-loader";
export type {
  VizJobResponse,
  VizLoadPhase,
  VizLoaderSink,
  VizLoadState,
} from "./model-loader";
export { RowName } from "./RowName";
export {
  createVisualizationPort,
  observeVisualizationJob,
} from "./visualization-port";
export type {
  JobObserverTarget,
  VisualizationPort,
} from "./visualization-port";
export { useLocalizedVizText } from "./use-viz-text";
export {
  createProductionVizText,
  useProductionVizText,
} from "./production-text";
export { createVizKeyText } from "./key-text";
export type { VisualizationViewRequest } from "./view-request";
