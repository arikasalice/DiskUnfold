import { describe, expect, test } from "vitest";
import {
  createProductionVizText,
  createVizKeyText,
  rawSourceSize,
  visualizationMessages,
} from "../src/viz";
import type { VizText } from "../src/viz";
import { bindVizMessage } from "../src/viz/use-viz-text";
import { realLocales } from "./locale-real";
import type { RealLocale } from "./locale-real";

const wireWords = [
  "complete",
  "partial",
  "missing",
  "cancelled",
  "current",
  "stale",
  "unknown",
  "scan",
  "current_analysis",
  "scope",
  "parent_folder",
  "analysis_total",
  "logical",
  "allocated",
  "unique_physical",
  "size",
  "treemap",
  "bar",
  "viz.flag.unknown",
  "viz.flag.lower_bound",
  "viz.flag.other_group",
  "unclassified",
  "ui.visualization.group.other",
  "ui.visualization.legend.unclassified",
];

function productionText(locale: RealLocale, unit: number | "auto" = "auto") {
  const message = bindVizMessage(locale.text);
  return {
    message,
    text: createProductionVizText(message, {
      system: "iec",
      unit,
      regional: locale.regional,
    }),
  };
}

const marker = "Translation unverified";

describe.each(["en", "ko"] as const)("production viz text (%s)", (language) => {
  const locale = realLocales[language];
  const { message, text } = productionText(locale);

  test("F06 FR9.HD1.gui-web.DD1: every message the viz layer uses is in the shipped bundle", () => {
    for (const key of Object.keys(visualizationMessages.en))
      expect(locale.keys.has(`ui.visualization.${key}`), key).toBe(true);
  });

  test("F06 FR9.HD1.gui-web.DD1: the status words are the words the analysis screen uses for the same state", () => {
    const sameAsAnalysis = [
      ["completenessComplete", "ui.analysis.version_final"],
      ["completenessPartial", "ui.analysis.version_interim"],
      ["completenessMissing", "ui.analysis.version_gaps"],
      ["completenessCancelled", "ui.analysis.scan_cancelled"],
      ["flagUnknown", "ui.analysis.unavailable"],
      ["unrecognizedValue", "ui.browse.availability_unknown"],
    ] as const;
    for (const [word, analysisKey] of sameAsAnalysis)
      expect(message(word), word).toBe(locale.text(analysisKey, ""));
  });

  test("F06 FR9.HD1.gui-web.DD1: the status words in the dictionary and in the bundle agree", () => {
    const statusWords = [
      "completenessComplete",
      "completenessPartial",
      "completenessMissing",
      "completenessCancelled",
      "denominatorScope",
      "denominatorAnalysisTotal",
      "flagLowerBound",
      "unrecognizedValue",
      "flagUnknown",
    ] as const;
    for (const word of statusWords)
      expect(visualizationMessages[language][word], word).toBe(message(word));
  });

  test("F06 FR9.HD1.gui-web.DD1: every wire word has its own text and none shows a key or a marker", () => {
    const general = message("unrecognizedValue");
    const shown = wireWords.map((word) => text.key(word));
    for (const [index, value] of shown.entries()) {
      expect(value, wireWords[index]).not.toContain(marker);
      expect(value, wireWords[index]).not.toContain("ui.visualization");
      expect(value, wireWords[index]).not.toContain("viz.flag");
      expect(value, wireWords[index]).not.toBe(general);
    }
    expect(text.key("ui.visualization.group.other")).toBe(
      language === "en" ? "Other items" : "기타 항목",
    );
  });

  test("F06 FR9.HD1.gui-web.DD1: a key outside the table gets the general phrase", () => {
    const general = message("unrecognizedValue");
    expect(general).not.toContain(marker);
    for (const key of [
      "ui.visualization.group.future",
      "scan.path_denied",
      "viz.flag.future",
      "constructor",
      "",
    ])
      expect(text.key(key), key).toBe(general);
  });

  test("F06 FR9.HD1.gui-web.DD1: sizes keep lower bounds and unknown values", () => {
    const exact = (bytes: string) =>
      text.size({
        state: "exact",
        bytes,
        basis: "logical",
        evidence: "measured",
      });
    expect(exact("1536")).toBe("1.5 KiB");
    expect(exact("1536")).not.toContain(marker);
    const atLeast = text.size({
      state: "lower_bound",
      bytes: "1536",
      basis: "logical",
      evidence: "estimated",
    });
    expect(atLeast).toBe(
      language === "en" ? "At least 1.5 KiB" : "최소 1.5 KiB",
    );
    expect(
      text.size({ state: "unknown", basis: "logical", evidence: "unknown" }),
    ).toBe(message("unavailableValue"));
    expect(
      text.quantity({ unit: "bytes", state: "exact", value: "1536" }),
    ).toBe("1.5 KiB");
    expect(text.quantity({ unit: "bytes", state: "exact", value: "1.5" })).toBe(
      message("unrecognizedValue"),
    );
  });

  test("F06 FR12.HD1.gui-web.DD1: item counts follow the language's plural forms", () => {
    const items = (value: string) =>
      text.quantity({ unit: "items", state: "exact", value });
    if (language === "en") {
      expect(items("1")).toBe("1 item");
      expect(items("2")).toBe("2 items");
      expect(items("1234")).toBe("1,234 items");
    } else {
      expect(items("1")).toBe("1개");
      expect(items("1234")).toBe("1,234개");
    }
    const atLeast = text.quantity({
      unit: "items",
      state: "lower_bound",
      value: "7",
    });
    expect(atLeast).toBe(language === "en" ? "At least 7 items" : "최소 7개");
    expect(
      text.quantity({ unit: "items", state: "unknown", reasonKey: "x" }),
    ).toBe(message("unavailableValue"));
    expect(text.quantity({ unit: "files", state: "exact", value: "3" })).toBe(
      message("unrecognizedValue"),
    );
  });

  test("F06 FR9.HD1.gui-web.DD1: the observation time uses the regional pattern", () => {
    expect(text.observedAt(null)).toBe(message("observedAtUnknown"));
    expect(text.observedAt("2026-11-14T03:04:05.000Z")).toBe(
      language === "en" ? "11/14/2026 03:04:05 UTC" : "2026.11.14 03:04:05 UTC",
    );
  });

  test("F06 FR12.HD1.gui-web.DD1: raw source sizes read as bytes, not as a rounded capacity", () => {
    const raw = (state: "exact" | "lower_bound") =>
      rawSourceSize(
        { state, bytes: "1234", basis: "logical", evidence: "measured" },
        message,
      );
    expect(raw("exact")).toBe(
      language === "en" ? "1,234 bytes · Measured" : "1,234바이트 · 측정됨",
    );
    expect(raw("lower_bound")).toBe(
      language === "en"
        ? "At least 1,234 bytes · Measured"
        : "최소 1,234바이트 · 측정됨",
    );
  });
});

test("F06 FR9.HD1.gui-web.DD1: the key text only needs a message function", () => {
  const key: VizText["key"] = createVizKeyText((name) => `<${name}>`);
  expect(key("complete")).toBe("<completenessComplete>");
  expect(key("nothing")).toBe("<unrecognizedValue>");
});
