import type {
  AnalysisClient,
  ItemRef,
  SizeValue,
  SpaceDetail,
} from "../src/ipc";
import { modelFixture } from "./model-fixture";

const resultVersion = "018f0000-0000-7000-8000-000000000001";
const ref = (itemNo: number): ItemRef => ({ resultVersion, itemNo });

export const sharedSpaceRefs = {
  folder: ref(1),
  counted: ref(2),
  notCounted: ref(3),
  large: ref(4),
};

const allocated = (bytes: number): SizeValue => ({
  state: "exact",
  bytes: String(bytes),
  basis: "allocated",
  evidence: "os_reported",
});
const logical = (bytes: number): SizeValue => ({
  state: "exact",
  bytes: String(bytes),
  basis: "logical",
  evidence: "measured",
});
const quantity = (bytes: number) => ({
  state: "exact" as const,
  value: String(bytes),
  unit: "bytes",
});
const total = (bytes: number) => ({
  quantity: quantity(bytes),
  sourceSize: allocated(bytes),
});
const present = <T>(value: T) => ({ state: "present" as const, value });

/** Same synthetic rows as engine-api/tests/viz_production.rs::hardlink_fixture. */
export function sharedSpaceModelFixture() {
  const { frame, page } = modelFixture();
  const { model } = frame;
  const baseShape = model.shapes[0]!;
  const baseRow = page.response.rows[0]!;
  const mib = 1024 * 1024;
  const denominator = {
    basis: "parent_folder" as const,
    value: quantity(3 * mib),
    size: allocated(3 * mib),
  };
  const shape = (
    itemRef: ItemRef,
    shapeRef: string,
    size: SizeValue,
    left: number,
    right: number,
  ) => ({
    ...baseShape,
    itemRef,
    groupRef: null,
    shapeRef,
    rootBranchRef: itemRef,
    itemKind: "file" as const,
    size,
    legendClassKey: "unclassified",
    displayFlags: [],
    geometry: {
      primitive: "rect",
      points: [
        { x: left, y: 0 },
        { x: right, y: 1_000_000 },
      ],
    },
  });

  frame.resultRevision = resultVersion;
  frame.completeness = "complete";
  model.modelRef = "hardlinks";
  model.aggregateScopeRef = sharedSpaceRefs.folder;
  model.navigationScopeRef = sharedSpaceRefs.folder;
  model.representation = {
    ...model.representation,
    format: "treemap",
    measure: "size",
    sizeBasis: "allocated",
    colorCriterion: "none",
    colorMode: "monochrome",
    labelMode: "name_size",
  };
  model.denominator = denominator;
  model.totals.scope = total(3 * mib);
  model.totals.drawn = total(3 * mib);
  model.shapes = [
    shape(sharedSpaceRefs.counted, "hardlink-a", allocated(mib), 0, 333_333),
    shape(
      sharedSpaceRefs.large,
      "large",
      allocated(2 * mib),
      337_333,
      1_000_000,
    ),
  ];

  page.request = {
    ...page.request,
    modelRef: model.modelRef,
    resultRevision: resultVersion,
    start: 0,
  };
  page.response.resultRevision = resultVersion;
  page.response.completeness = "complete";
  page.response.sizeBasis = "allocated";
  page.response.totalCount = 3;
  page.response.rows = [
    {
      ...baseRow,
      itemRef: sharedSpaceRefs.counted,
      groupRef: null,
      displayName: "a",
      itemKind: "file",
      size: allocated(mib),
      ratio: "0.33333333333333333333",
      denominator,
      legendClassKey: "unclassified",
      displayFlags: [],
      sharesSpace: true,
    },
    {
      ...baseRow,
      itemRef: sharedSpaceRefs.notCounted,
      groupRef: null,
      displayName: "b",
      itemKind: "file",
      size: allocated(0),
      ratio: null,
      denominator,
      legendClassKey: "unclassified",
      displayFlags: [],
      sharesSpace: true,
      sharedCountedItem: sharedSpaceRefs.counted,
    },
    {
      ...baseRow,
      itemRef: sharedSpaceRefs.large,
      groupRef: null,
      displayName: "large",
      itemKind: "file",
      size: allocated(2 * mib),
      ratio: "0.66666666666666666667",
      denominator,
      legendClassKey: "unclassified",
      displayFlags: [],
    },
  ];
  return { frame, page };
}

const unavailable = { state: "absent" } as const;
const sharedSpaceDetail: SpaceDetail = {
  item: sharedSpaceRefs.notCounted,
  logical: present({ size: logical(1024 * 1024), measurementLimits: [] }),
  allocated: present({
    size: allocated(1024 * 1024),
    measurementLimits: [],
  }),
  uniquePhysical: unavailable,
  exclusive: unavailable,
  shared: unavailable,
  linkGroup: present({
    reportedLinkCount: "2",
    inScopePaths: [sharedSpaceRefs.counted, sharedSpaceRefs.notCounted],
    sharesSpace: true,
    countedItem: sharedSpaceRefs.counted,
    countedFolder: sharedSpaceRefs.folder,
    hasOutsideLinks: false,
  }),
  auxiliary: unavailable,
  exclusiveDecision: { confirmed: false, reasonKey: null },
  observedAt: unavailable,
  capabilities: unavailable,
};

/** A local, fixed client for the two reads used by ItemSpaceDetails. */
export const sharedSpaceClient = {
  async call(name: string) {
    if (name === "space.detail") return sharedSpaceDetail;
    if (name === "browse.detail")
      return {
        displayPath: {
          state: "present",
          value: "C:\\Users\\민서\\문서\\files\\a",
        },
      };
    throw new Error("Unexpected preview request: " + name);
  },
} as unknown as AnalysisClient;
