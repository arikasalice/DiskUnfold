import {
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
  VizRows,
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
const names = ["Videos", OTHER_GROUP_LABEL, "Photos", "Documents"];

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
  model.shapes.forEach((shape, index) => {
    const otherGroup = index === 0;
    shape.itemRef = otherGroup ? null : ref(index + 1);
    shape.groupRef = otherGroup ? "group-other" : null;
    shape.rootBranchRef = otherGroup ? null : ref(index + 1);
    shape.legendClassKey = "unclassified";
  });
  return frame;
}

function rowsFor(
  request: VizRenderRequest | undefined,
  revision: string,
): VizRows {
  const frame = request ? frameFor(request) : modelFixture().frame;
  return {
    rows: frame.model.shapes.map((shape, index) => {
      const otherGroup = shape.groupRef !== null && shape.itemRef === null;
      return {
        itemRef: shape.itemRef,
        groupRef: shape.groupRef,
        displayName: otherGroup ? OTHER_GROUP_LABEL : (names[index] ?? "Item"),
        ...(otherGroup && { labelKey: OTHER_GROUP_LABEL }),
        size: shape.size,
        ratio: "0.25",
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

function desktop(language: "en" | "ko") {
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
  const answer = (envelope: CallEnvelope): unknown => {
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
        return frameFor(lastRequest);
      case "viz.rows":
        return rowsFor(lastRequest, body.resultRevision as string);
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
      body: answer(envelope),
    };
  });
  return { client, calls };
}

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
    const groupLabel = language === "en" ? "Other (0)" : "기타 0개";
    expect(await screen.findAllByText(groupLabel)).not.toHaveLength(0);
    expect(await screen.findAllByText(OTHER_GROUP_LABEL)).not.toHaveLength(0);

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
      depthLimit: 1,
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
    expect(
      Array.from(
        shell?.querySelectorAll<HTMLElement>("button[aria-label]") ?? [],
      ).some((element) =>
        element.getAttribute("aria-label")?.startsWith(`${groupLabel} ·`),
      ),
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
