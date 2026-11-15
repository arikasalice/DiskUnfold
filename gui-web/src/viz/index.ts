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
export type { VizText } from "./model-text";
export {
  visualizationFormatName,
  visualizationMessage,
  visualizationMessages,
  visualizationReason,
} from "./messages";
export type {
  VisualizationLanguage,
  VisualizationMessageKey,
} from "./messages";
export { RowName } from "./RowName";
