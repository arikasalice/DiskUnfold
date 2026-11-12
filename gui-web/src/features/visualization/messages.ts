export const visualizationMessages = {
  en: {
    loading: "Loading visualization",
    working: "Visualization work is in progress.",
    cancelled:
      "Visualization was cancelled. The previous graph remains available when present.",
    renderFailed: "Visualization could not be updated.",
    retryRender: "Retry visualization",
    retryingRender: "Retrying visualization…",
    graphUpdated: "Visualization result updated.",
    unavailable: "Visualization is unavailable:",
    formats: "Available formats:",
    noFormats: "None supplied",
    selectionLost:
      "The previous selection could not be matched to this result.",
    focusLost: "The previously focused item is unavailable in this graph.",
    focusGraphAction: "Choose another item in the graph",
    noGraphTargets: "There are no selectable items in the graph.",
    previousName: "Previous item:",
    previousBasis: "Previous observation:",
    previousMeasure: "Size basis:",
    correspondenceUnknown:
      "The reason the item could not be matched is unavailable.",
    nextSelection:
      "When the current result's item list is available, review it and select the item again.",
    newerAvailable:
      "A newer result is available. This graph remains pinned to the earlier observation.",
    reviewNewer: "Review newer result",
    revisionStale:
      "A displayed result is no longer available for lookup. Reopen the visualization to review the selected result.",
    revisionUnverified:
      "Could not recheck a displayed result. Reopen the visualization to check the selected result.",
    newerRegion: "Items in this result",
    loadNewer: "Load items in this result",
    loadingNewer: "Loading items in this result",
    newerTitle: "Items in this result",
    retainedGraph:
      "This result cannot be displayed as a graph here, so the previous graph remains displayed. Graph observed:",
    newTableObservation: "The list belongs to this result. List observed:",
    sameRevision:
      "The graph and list show the same observation in different ways.",
    differentRevision: "The graph and list have different observation bases.",
    rowsFailed: "Rows could not be loaded.",
    retryRows: "Retry rows",
    retryingRows: "Retrying rows…",
    graphRowsStatus: "Rows for the displayed graph",
    newerRowsStatus: "Items in this result",
    noBaseline: "No comparison baseline is available",
    singleObservation: "More than one observation is required",
    noAttribution: "Space attribution is unavailable",
    noItemSizes: "Item sizes are unavailable",
    negativeFlow: "Negative flow cannot be displayed in this format",
    unknownReason: "The reason is unavailable",
    unknownFormat: "Unknown format",
    treemap: "Treemap",
    bar: "Bar chart",
    trend: "Trend",
    waterfall: "Waterfall",
    histogram: "Histogram",
    sankey: "Sankey",
  },
  ko: {
    loading: "시각화 불러오는 중",
    working: "시각화 작업이 진행 중입니다.",
    cancelled:
      "시각화가 취소되었습니다. 이전 그래프가 있으면 계속 볼 수 있습니다.",
    renderFailed: "시각화를 갱신하지 못했습니다.",
    retryRender: "시각화 다시 시도",
    retryingRender: "시각화 다시 시도 중…",
    graphUpdated: "시각화 결과를 갱신했습니다.",
    unavailable: "시각화를 사용할 수 없습니다:",
    formats: "사용 가능한 형식:",
    noFormats: "제공되지 않음",
    selectionLost:
      "이전 선택에 대응하는 항목을 이 결과에서 확인하지 못했습니다.",
    focusLost: "이전에 초점이 있던 항목을 이 그래프에서 사용할 수 없습니다.",
    focusGraphAction: "그래프에서 다른 항목 선택",
    noGraphTargets: "그래프에서 선택할 수 있는 항목이 없습니다.",
    previousName: "이전 항목:",
    previousBasis: "이전 관측:",
    previousMeasure: "크기 기준:",
    correspondenceUnknown: "항목을 대응하지 못한 이유는 확인할 수 없습니다.",
    nextSelection:
      "현재 결과의 항목 목록을 사용할 수 있으면 검토하고 다시 선택하세요.",
    newerAvailable:
      "새 결과가 있습니다. 현재 그래프는 이전 관측에 고정되어 있습니다.",
    reviewNewer: "새 결과 검토",
    revisionStale:
      "표시된 결과를 더 이상 조회할 수 없습니다. 선택한 결과를 보려면 시각화를 다시 여세요.",
    revisionUnverified:
      "표시된 결과를 다시 확인하지 못했습니다. 선택한 결과를 확인하려면 시각화를 다시 여세요.",
    newerRegion: "이번 결과의 항목",
    loadNewer: "이번 결과의 항목 불러오기",
    loadingNewer: "이번 결과의 항목 불러오는 중",
    newerTitle: "이번 결과의 항목",
    retainedGraph:
      "이번 결과를 이곳에서 그래프로 표시할 수 없어 이전 그래프를 유지합니다. 그래프 관측 시각:",
    newTableObservation: "목록은 이번 결과에 속합니다. 목록 관측 시각:",
    sameRevision: "그래프와 목록은 같은 관측 결과를 다른 방식으로 보여줍니다.",
    differentRevision: "그래프와 목록의 관측 기준이 다릅니다.",
    rowsFailed: "행을 불러오지 못했습니다.",
    retryRows: "행 다시 시도",
    retryingRows: "행 다시 시도 중…",
    graphRowsStatus: "표시된 그래프의 행",
    newerRowsStatus: "이번 결과의 항목",
    noBaseline: "비교 기준이 없습니다",
    singleObservation: "관측이 두 개 이상 필요합니다",
    noAttribution: "공간 귀속 자료를 사용할 수 없습니다",
    noItemSizes: "항목 크기를 사용할 수 없습니다",
    negativeFlow: "이 형식에서는 음수 흐름을 표시할 수 없습니다",
    unknownReason: "사유를 확인할 수 없습니다",
    unknownFormat: "알 수 없는 형식",
    treemap: "트리맵",
    bar: "막대 그래프",
    trend: "추세선",
    waterfall: "워터폴",
    histogram: "히스토그램",
    sankey: "생키",
  },
} as const;
export type VisualizationMessageKey = keyof typeof visualizationMessages.en;
export type VisualizationLanguage = keyof typeof visualizationMessages;
export function visualizationMessage(
  language: VisualizationLanguage,
  key: VisualizationMessageKey,
): string {
  return visualizationMessages[language][key];
}
const formats: Record<string, VisualizationMessageKey> = {
  treemap: "treemap",
  bar: "bar",
  trend: "trend",
  waterfall: "waterfall",
  histogram: "histogram",
  sankey: "sankey",
};
export function visualizationFormatName(
  format: string,
  message: (key: VisualizationMessageKey) => string,
): string {
  return message(formats[format.replace(/^viz\./, "")] ?? "unknownFormat");
}
const reasons: Record<string, VisualizationMessageKey> = {
  "viz.unavailable.no_baseline": "noBaseline",
  "viz.unavailable.single_observation": "singleObservation",
  "viz.unavailable.no_attribution": "noAttribution",
  "viz.unavailable.no_item_sizes": "noItemSizes",
  "viz.unavailable.negative_flow": "negativeFlow",
};
export function visualizationReason(
  reason: string,
  message: (key: VisualizationMessageKey) => string,
): string {
  return message(reasons[reason] ?? "unknownReason");
}
