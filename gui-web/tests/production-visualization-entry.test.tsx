import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { DesktopLocale } from "../src/app/DesktopLocale";
import { createAnalysisClient, windowsPath } from "../src/ipc";
import type {
  CallEnvelope,
  LocaleProfile,
  VizCatalog,
  VizRenderRequest,
  VizRenderResponse,
  VizRows,
  VizRowsRequest,
} from "../src/ipc";
import { modelFixture } from "../preview/model-fixture";
import { realLocales } from "./locale-real";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const stamp = "2026-11-15T00:00:00.000Z";
const target = windowsPath("C:\\fixture");
const OTHER_GROUP_LABEL = "ui.visualization.group.other";
const catalog: VizCatalog = {
  formats: ["treemap", "bar", "histogram"],
  strategies: [{ id: "viz.treemap", revision: 1 }],
  measures: ["size"],
  colorCriteria: ["none"],
  denominatorCriteria: ["scope"],
  interpretationTableRevision: 1,
  requiredDataTableRevision: 1,
  legendRuleRevision: 1,
};
const names = ["Videos", "Photos", "Documents"];

type VisualizationAnswers = {
  render?: (request: VizRenderRequest) => unknown | Promise<unknown>;
  rows?: (request: VizRowsRequest) => unknown | Promise<unknown>;
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function frameFor(request: VizRenderRequest) {
  const { frame } = modelFixture();
  const revision = request.resultRevision ?? "missing";
  const ref = (itemNo: number) => ({ resultVersion: revision, itemNo });
  const model = frame.model;
  frame.requestSeq = request.requestSeq;
  frame.resultRevision = revision;
  frame.completeness = "complete";
  model.aggregateScopeRef = request.aggregateScopeRef;
  model.navigationScopeRef = request.navigationScopeRef;
  model.origin = "scan";
  model.staleness = { state: "unknown", reasonKey: "viz.staleness.unwatched" };
  model.representation = {
    format: "treemap",
    measure: "size",
    sizeBasis: request.sizeBasis,
    classificationCriterion: "none",
    colorCriterion: "none",
    denominatorBasis: "scope",
    colorMode: null,
    labelMode: null,
  };
  model.legend = [
    {
      key: "unclassified",
      labelKey: "ui.visualization.legend.unclassified",
      itemCount: { unit: "items", state: "exact", value: "4" },
      total: model.totals.scope,
      condition: null,
    },
  ];
  const groupedShape = model.shapes[0]!;
  if (groupedShape.size.state === "unknown")
    throw new Error("The grouped fixture needs a known size");
  model.summary.groupedItems = { state: "exact", value: "1", unit: "items" };
  model.totals.grouped = {
    quantity: {
      state: groupedShape.size.state,
      value: groupedShape.size.bytes,
      unit: "bytes",
    },
    sourceSize: groupedShape.size,
  };
  model.shapes.forEach((shape, index) => {
    const otherGroup = index === 0;
    shape.itemRef = otherGroup ? null : ref(index + 1);
    shape.groupRef = otherGroup ? "group-other" : null;
    shape.matchKey = otherGroup
      ? { state: "not_applicable" }
      : { state: "unknown", reasonKey: "fixture.identity_unavailable" };
    shape.rootBranchRef = otherGroup ? null : ref(index + 1);
    shape.legendClassKey = "unclassified";
  });
  return frame;
}

function rowsFor(
  request: VizRenderRequest | undefined,
  revision: string,
): VizRows {
  const fixture = modelFixture();
  const frame = request ? frameFor(request) : fixture.frame;
  return {
    rows: frame.model.shapes.map((shape, index) => {
      const otherGroup = shape.groupRef !== null && shape.itemRef === null;
      return {
        itemRef: shape.itemRef,
        groupRef: shape.groupRef,
        displayName: otherGroup
          ? OTHER_GROUP_LABEL
          : (names[index - 1] ?? "Item"),
        ...(otherGroup && { labelKey: OTHER_GROUP_LABEL }),
        size: shape.size,
        ratio:
          index === 1 ? "-0.00004" : fixture.page.response.rows[index]!.ratio,
        denominator: frame.model.denominator,
        legendClassKey: "unclassified",
        displayFlags: [],
      };
    }),
    totalCount: 4,
    resultRevision: revision,
    observedAt: request ? frame.observedAt : null,
    completeness: "complete",
  };
}

function desktop(
  language: "en" | "ko",
  visualization: VisualizationAnswers = {},
) {
  const supported = (["en", "ko"] as const).map((code) => ({
    language: code,
    selfNameKey: `locale.language.${code}`,
    bundleRevision: code,
  }));
  const profile: LocaleProfile = {
    displayLanguage: language,
    languageSource: "os",
    dateFormat: "lang_default",
    numberFormat: "lang_default",
    timeZonePolicy: "utc",
    localTimeZoneStatus: "unavailable",
    motion: "follow_os",
    appScale: 100,
    profileRevision: "1",
    supportedLanguages: supported,
  };
  const calls: { name: string; body: unknown }[] = [];
  let lastRequest: VizRenderRequest | undefined;
  const answer = (envelope: CallEnvelope): unknown | Promise<unknown> => {
    const body = envelope.body as Record<string, unknown>;
    switch (envelope.name) {
      case "i18n.get_profile":
        return profile;
      case "i18n.get_bundle": {
        const code = body.language as "en" | "ko";
        return { bundleRevision: code, bundle: realLocales[code].body };
      }
      case "space.display_preferences":
        return {
          settingsRevision: "0",
          unitSystem: "iec",
          unit: "auto",
          defaultBasis: "logical",
          warningKey: null,
        };
      case "target.list":
        return {
          targets: [
            {
              kind: "internal",
              path: target,
              connected: true,
              volumeId: "volume",
              identitySource: "confirmed",
              access: "accessible",
              reasonKey: null,
            },
          ],
          probeJobRef: null,
          observedAt: stamp,
          unavailableReason: null,
        };
      case "scope.review":
        return {
          scope: body.draft,
          notices: [],
          targets: (body.draft as { includes: unknown[] }).includes.map(
            (path) => ({ path, access: "accessible", reasonKey: null }),
          ),
          reviewFingerprint: "verified",
          observedAt: stamp,
          requestSeq: body.requestSeq,
        };
      case "scan.start":
        return {
          jobRef: "scan",
          existing: false,
          scopeVersion: "scope",
          scopeFingerprint: "verified",
        };
      case "job.get":
        return {
          jobRef: "scan",
          kind: "scan.metadata",
          state: "done",
          sequence: 3,
          resultVersion: "result",
          reasonKey: null,
        };
      case "job.list":
        return [];
      case "space.total":
        return {
          resultVersion: body.resultVersion,
          requestSeq: body.requestSeq,
          completeness: "complete",
          sizes: [
            {
              size: {
                basis: "logical",
                evidence: "measured",
                state: "exact",
                bytes: "42",
              },
              completeness: "complete",
              measurementLimits: [],
            },
          ],
          missingItems: "0",
          inaccessibleRanges: null,
          untraversedLinks: "0",
        };
      case "browse.open":
        return {
          kind: "opened",
          requestSeq: body.requestSeq,
          origin: "scan",
          location: { resultVersion: "result", itemNo: 5 },
          breadcrumbs: [
            {
              item: { resultVersion: "result", itemNo: 5 },
              displayName: { state: "present", value: "FolderOfVideos" },
            },
          ],
          rootKind: "scope_root",
          currentCorrespondence: false,
          page: {
            rowKind: "folder",
            queryRef: "folder-query",
            resultVersion:
              (body.resultVersion as string | undefined) ?? "result",
            rows: [],
            total: { state: "exact", count: 0 },
            unknownCount: 0,
            observedAt: stamp,
            completeness: "complete",
            nextCursor: null,
          },
          sortStrategies: [],
          predicateStrategies: [],
        };
      case "query.region.open":
        return { regionRef: "browse-region" };
      case "query.cancel":
        return { outcome: "no_effect" };
      case "viz.catalog":
        return catalog;
      case "viz.view.open":
        return { viewChannel: "view" };
      case "viz.render":
        lastRequest = body as unknown as VizRenderRequest;
        return visualization.render?.(lastRequest) ?? frameFor(lastRequest);
      case "viz.rows": {
        const request = body as unknown as VizRowsRequest;
        return (
          visualization.rows?.(request) ??
          rowsFor(lastRequest, request.resultRevision)
        );
      }
      case "viz.cancel":
        return { controlResult: "no_effect" };
      default:
        return {};
    }
  };
  const client = createAnalysisClient(async (_command, { envelope }) => {
    calls.push({ name: envelope.name, body: envelope.body });
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: 1,
      body: await answer(envelope),
    };
  });
  return { client, calls };
}

test.each(["success", "failure"] as const)(
  "F06 #109: prior ordinary row %s leaves the fallback auto-read pending",
  async (ordinaryOutcome) => {
    const ordinaryRows = deferred<VizRows>();
    const fallbackReads: ReturnType<typeof deferred<VizRows>>[] = [];
    const fallbackRequests: VizRowsRequest[] = [];
    let firstRender: VizRenderRequest | undefined;
    let renderCount = 0;
    const { client, calls } = desktop("en", {
      render: (request) => {
        renderCount += 1;
        if (renderCount === 1) {
          firstRender = request;
          return frameFor(request);
        }
        return {
          kind: "unavailable",
          requestSeq: request.requestSeq,
          resultRevision: request.resultRevision ?? "result",
          observedAt: null,
          completeness: "partial",
          reasonKey: "viz.unavailable.no_baseline",
          availableFormats: [],
          fallback: {
            modelRef: "fallback-model",
            resultRevision: request.resultRevision ?? "result",
          },
        } satisfies VizRenderResponse;
      },
      rows: (request) => {
        if (request.modelRef === "synthetic-layout")
          return ordinaryRows.promise;
        const read = deferred<VizRows>();
        fallbackRequests.push(request);
        fallbackReads.push(read);
        return read.promise;
      },
    });
    const label = (key: string) => realLocales.en.text(key, "");
    render(<DesktopLocale client={client} />);
    fireEvent.click(await screen.findByRole("radio", { name: target.display }));
    fireEvent.click(
      screen.getByRole("button", { name: label("ui.analysis.review") }),
    );
    const start = await screen.findByRole<HTMLButtonElement>("button", {
      name: label("ui.analysis.start"),
    });
    await waitFor(() => expect(start.disabled).toBe(false));
    fireEvent.click(start);
    fireEvent.click(
      await screen.findByRole("button", { name: label("ui.analysis.browse") }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: label("ui.visualization.show_space_distribution"),
      }),
    );
    await screen.findByRole("group", { name: "Space distribution" });
    const ordinaryRequests = () =>
      calls
        .filter(({ name }) => name === "viz.rows")
        .map(({ body }) => body as VizRowsRequest)
        .filter(({ modelRef }) => modelRef === "synthetic-layout");
    await waitFor(() => expect(fallbackRequests).toHaveLength(1));
    expect(ordinaryRequests().length).toBeGreaterThan(0);
    for (const request of ordinaryRequests())
      expect(request).toMatchObject({
        modelRef: "synthetic-layout",
        resultRevision: "result",
        start: 0,
        count: 100,
      });
    const fallbackRequest = {
      modelRef: "fallback-model",
      resultRevision: "result",
      start: 0,
      count: 100,
    };
    expect(fallbackRequests[0]).toEqual(fallbackRequest);
    const rowCallBodies = calls
      .filter(({ name }) => name === "viz.rows")
      .map(({ body }) => body as VizRowsRequest);
    expect(
      rowCallBodies.findIndex(({ modelRef }) => modelRef === "fallback-model"),
    ).toBeGreaterThan(
      rowCallBodies.findIndex(
        ({ modelRef }) => modelRef === "synthetic-layout",
      ),
    );
    expect(
      screen.getByRole("progressbar", {
        name: "Loading items in this folder",
      }),
    ).toBeTruthy();

    await act(async () => {
      if (ordinaryOutcome === "success")
        ordinaryRows.resolve(rowsFor(firstRender!, "result"));
      else ordinaryRows.reject(new Error("previous ordinary rows failed"));
    });
    const afterOrdinaryRows = {
      fallbackRequests: fallbackRequests.length,
      loading:
        screen.queryByRole("progressbar", {
          name: "Loading items in this folder",
        }) !== null,
      retry: screen.queryByRole("button", { name: "Retry rows" }) !== null,
    };
    for (const staleRead of fallbackReads.slice(0, -1))
      await act(async () => staleRead.resolve(rowsFor(firstRender!, "result")));

    await act(async () =>
      fallbackReads[fallbackReads.length - 1]!.reject(
        new Error("latest fallback failed"),
      ),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Retry rows" }));
    await screen.findByRole("button", { name: "Retrying rows…" });
    await waitFor(() =>
      expect(fallbackReads).toHaveLength(
        afterOrdinaryRows.fallbackRequests + 1,
      ),
    );
    expect(fallbackRequests[fallbackRequests.length - 1]).toEqual(
      fallbackRequest,
    );
    expect(
      screen.getByRole("progressbar", {
        name: "Loading items in this folder",
      }),
    ).toBeTruthy();

    const retryResponse = rowsFor(firstRender!, "result");
    const recoveredRow = retryResponse.rows.find(
      (row) => row.itemRef !== null,
    )!;
    await act(async () =>
      fallbackReads[fallbackReads.length - 1]!.resolve({
        ...retryResponse,
        rows: [{ ...recoveredRow, displayName: "Recovered fallback row" }],
        totalCount: 1,
      }),
    );
    expect(await screen.findByText("Recovered fallback row")).toBeTruthy();
    expect(
      screen.queryByRole("progressbar", {
        name: "Loading items in this folder",
      }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry rows" })).toBeNull();
    expect(afterOrdinaryRows).toEqual({
      fallbackRequests: 1,
      loading: true,
      retry: false,
    });
    expect(fallbackRequests).toHaveLength(2);
  },
);

beforeEach(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  const original = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      return this.matches(".du-visualization-viewport")
        ? new DOMRect(0, 0, 640, 400)
        : original.call(this);
    },
  );
});

test("F06 FR12.HD1.gui-web.DD1: a depth 2 treemap names the shapes inside a folder from that folder's rows", async () => {
  // Shape 1 is a folder drawn at depth 1; the engine also drew one file and one
  // group inside it. Their names come only from the folder's own table.
  const nestedFrame = (request: VizRenderRequest) => {
    const frame = frameFor(request);
    const revision = frame.resultRevision;
    const shapes = frame.model.shapes;
    shapes.forEach((shape, index) => {
      shape.shapeRef = `s${index}`;
      shape.parentShapeRef = null;
      shape.visibleDepth = 1;
    });
    const folder = shapes[1]!;
    shapes.push(
      {
        ...folder,
        itemRef: { resultVersion: revision, itemNo: 40 },
        shapeRef: "nested-file",
        parentShapeRef: "s1",
        visibleDepth: 2,
      },
      {
        ...folder,
        itemRef: null,
        groupRef: "nested-other",
        matchKey: { state: "not_applicable" },
        shapeRef: "nested-other",
        parentShapeRef: "s1",
        visibleDepth: 2,
        memberCount: { unit: "items", state: "lower_bound", value: "7" },
      },
    );
    return frame;
  };
  let lastFrame: ReturnType<typeof nestedFrame> | undefined;
  const folderReads: VizRowsRequest[] = [];
  const { client, calls } = desktop("en", {
    render: (request) => (lastFrame = nestedFrame(request)),
    rows: (request) => {
      const frame = lastFrame!;
      if (request.parentShapeRef === undefined) {
        const root = rowsFor(undefined, request.resultRevision);
        root.observedAt = frame.observedAt;
        root.rows = frame.model.shapes
          .filter((shape) => !shape.parentShapeRef)
          .map((shape, index) => ({
            ...root.rows[index]!,
            itemRef: shape.itemRef,
            groupRef: shape.groupRef,
            ...(shape.groupRef
              ? {
                  memberCount: {
                    unit: "items",
                    state: "exact" as const,
                    value: "1",
                  },
                }
              : {}),
          }));
        return root;
      }
      folderReads.push(request);
      const [file, group] = frame.model.shapes.slice(-2);
      return {
        rows: [
          {
            itemRef: file!.itemRef,
            groupRef: null,
            displayName: "Holiday.mp4",
            size: file!.size,
            ratio: "0.5",
            denominator: frame.model.denominator,
            legendClassKey: "unclassified",
            displayFlags: [],
          },
          {
            itemRef: null,
            groupRef: group!.groupRef,
            displayName: OTHER_GROUP_LABEL,
            labelKey: OTHER_GROUP_LABEL,
            memberCount: group!.memberCount!,
            size: group!.size,
            ratio: "0.1",
            denominator: frame.model.denominator,
            legendClassKey: "unclassified",
            displayFlags: [],
          },
        ],
        totalCount: 2,
        resultRevision: request.resultRevision,
        observedAt: frame.observedAt,
        completeness: "complete",
      } satisfies VizRows;
    },
  });
  const label = (key: string) => realLocales.en.text(key, "");
  render(<DesktopLocale client={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: target.display }));
  fireEvent.click(
    screen.getByRole("button", { name: label("ui.analysis.review") }),
  );
  const start = await screen.findByRole<HTMLButtonElement>("button", {
    name: label("ui.analysis.start"),
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  fireEvent.click(
    await screen.findByRole(
      "button",
      { name: label("ui.analysis.browse") },
      { timeout: 5000 },
    ),
  );
  fireEvent.click(
    await screen.findByRole(
      "button",
      { name: label("ui.visualization.show_space_distribution") },
      { timeout: 5000 },
    ),
  );
  await screen.findByRole(
    "button",
    { name: /^Holiday\.mp4 · / },
    {
      timeout: 5000,
    },
  );
  await screen.findByRole("button", { name: /^Other \(7\+\) · / });
  expect(calls.find(({ name }) => name === "viz.render")?.body).toMatchObject({
    depthLimit: 2,
  });
  expect(folderReads).toEqual([
    {
      modelRef: lastFrame!.model.modelRef,
      resultRevision: "result",
      start: 0,
      count: 2,
      parentShapeRef: "s1",
    },
  ]);
});

describe.each(["en", "ko"] as const)("the product shell (%s)", (language) => {
  const locale = realLocales[language];
  const label = (key: string) => locale.text(key, "");
  // A complete, current-analysis result has no badge; the unwatched result says so once.
  const expected = {
    en: ["Changes made after the analysis aren't reflected"],
    ko: ["분석한 뒤에 바뀐 내용은 반영되지 않습니다"],
  }[language];
  const badgeWords = {
    en: ["Final", "Current analysis", "Freshness unknown"],
    ko: ["최종", "현재 분석", "최신 여부 확인 불가"],
  }[language];
  const smallNegativeRatio = {
    en: "Greater than -0.1% and less than 0%",
    ko: "-0.1% 초과 0% 미만",
  }[language];

  test("F06 FR10.HD1.app-host.DD2: after an analysis the folder view opens a treemap from the real provider", async () => {
    const { client, calls } = desktop(language);
    render(<DesktopLocale client={client} />);
    fireEvent.click(await screen.findByRole("radio", { name: target.display }));
    fireEvent.click(
      screen.getByRole("button", { name: label("ui.analysis.review") }),
    );
    const start = await screen.findByRole<HTMLButtonElement>("button", {
      name: label("ui.analysis.start"),
    });
    await waitFor(() => expect(start.disabled).toBe(false));
    fireEvent.click(start);
    fireEvent.click(
      await screen.findByRole(
        "button",
        { name: label("ui.analysis.browse") },
        { timeout: 5000 },
      ),
    );
    const open = await screen.findByRole(
      "button",
      { name: label("ui.visualization.show_space_distribution") },
      { timeout: 5000 },
    );
    expect(calls.some(({ name }) => name === "viz.render")).toBe(false);

    fireEvent.click(open);
    await screen.findByRole("button", { name: /Photos/ }, { timeout: 5000 });
    fireEvent.click(
      await screen.findByRole("button", {
        name: label("ui.visualization.rowsFromGraph"),
      }),
    );
    const groupLabel = language === "en" ? "Other (1)" : "기타 1개";

    const request = calls.find(({ name }) => name === "viz.render");
    expect(request?.body).toMatchObject({
      viewChannel: "view",
      resultRevision: "result",
      format: "treemap",
      aggregateScopeRef: { resultVersion: "result", itemNo: 5 },
      navigationScopeRef: { resultVersion: "result", itemNo: 5 },
      measure: "size",
      sizeBasis: "logical",
      classificationCriterion: "none",
      colorCriterion: "none",
      denominatorBasis: "scope",
      depthLimit: 2,
      viewport: { width: 640, height: 296, scale: 1 },
    });
    expect(request?.body).not.toHaveProperty("strategy");
    expect(request?.body).not.toHaveProperty("focusItemRef");
    expect(calls.some(({ name }) => name === "viz.rows")).toBe(true);

    const shell = document.querySelector<HTMLElement>(
      ".du-visualization-shell",
    );
    expect(shell).not.toBeNull();
    const shown = shell?.textContent ?? "";
    for (const word of expected) expect(shown).toContain(word);
    for (const name of names) expect(shown).toContain(name);
    expect(shown).toContain(groupLabel);
    expect(shown).not.toContain(OTHER_GROUP_LABEL);
    const accessibleLabels = Array.from(
      shell?.querySelectorAll<HTMLElement>("button[aria-label]") ?? [],
      (element) => element.getAttribute("aria-label") ?? "",
    );
    expect(
      accessibleLabels.some((name) => name.startsWith(`${groupLabel} ·`)),
    ).toBe(true);
    for (const name of names)
      expect(accessibleLabels.some((label) => label.includes(name))).toBe(true);
    expect(
      accessibleLabels.some((label) => label.includes(smallNegativeRatio)),
    ).toBe(true);
    expect(shown).not.toContain(label("ui.visualization.freshnessUnknown"));
    // The view names itself and the folder it shows.
    expect(shown).toContain(label("ui.visualization.space_distribution"));
    expect(shown).toContain("FolderOfVideos");
    for (const word of badgeWords) expect(shown).not.toContain(word);
    expect(shown).not.toContain("Translation unverified");
    expect(shown).not.toContain("viz.");

    await new Promise((resolve) => setTimeout(resolve, 60));
    const missing = calls
      .filter(({ name }) => name === "i18n.report_missing")
      .flatMap(({ body }) => (body as { entries: { key: string }[] }).entries)
      .filter(({ key }) => key.startsWith("ui.visualization."));
    expect(missing).toEqual([]);
  });
});
