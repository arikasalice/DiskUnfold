import { Heading, Readout } from "../src/components";
import { ItemSpaceDetails } from "../src/features/analysis/ItemSpaceDetails";
import { LocaleTextContext } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import { windowsSizeDisplay } from "../src/features/analysis/space-format";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import { englishText, koreanText } from "./result-summary-fixture";
import {
  sharedSpaceClient,
  sharedSpaceModelFixture,
  sharedSpaceRefs,
} from "./shared-space-fixture";
import "../src/theme/analysis-results.css";
import "../src/theme/analysis-workspace.css";

const regional = {
  language: "ko-KR",
  decimalSeparator: ".",
  groupingSeparator: ",",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "lang_default" as const,
  groupNumbers: true,
  timeZonePolicy: "local" as const,
  localTimeZoneStatus: "identified" as const,
};

/** Fixed Windows browser scene for the F03 hard-link review. */
export function SharedSpacePreview({ scene }: { scene: string }) {
  const locale = new URLSearchParams(window.location.search).get("locale");
  const localeText = locale === "en" ? englishText : koreanText;
  const { frame, page } = sharedSpaceModelFixture();
  const text: VizText = createProductionVizText(
    (key, values) =>
      Object.entries(values ?? {}).reduce(
        (message, [name, value]) =>
          message.replace("{" + name + "}", String(value)),
        visualizationMessage(locale === "en" ? "en" : "ko", key),
      ),
    { system: "iec", unit: "auto" },
  );

  return (
    <LocaleTextContext.Provider value={localeText}>
      <RegionalFormatContext.Provider value={regional}>
        <main className="du-shared-space-preview" style={{ padding: 24 }}>
          {scene === "detail" ? (
            <section className="du-review-stack du-shared-space-preview__detail">
              <Heading level={2}>
                {locale === "en" ? "File details" : "파일 세부 정보"}
              </Heading>
              <Readout
                label={locale === "en" ? "File" : "파일"}
                rows={[
                  {
                    label: locale === "en" ? "Name" : "이름",
                    value: "b",
                  },
                  {
                    label: locale === "en" ? "Allocated size" : "할당 크기",
                    value: "1 MiB",
                  },
                ]}
              />
              <ItemSpaceDetails
                client={sharedSpaceClient}
                item={sharedSpaceRefs.notCounted}
                display={windowsSizeDisplay}
                navigateItem={() => {}}
              />
            </section>
          ) : (
            <section className="du-shared-space-preview__table">
              <Heading level={2}>
                {locale === "en" ? "Storage visualization" : "저장 공간 시각화"}
              </Heading>
              <VisualizationModelPresenter
                frame={frame}
                page={page}
                view={{
                  viewport: {
                    width: Math.max(320, window.innerWidth - 48),
                    height: 300,
                    scale: 1,
                  },
                  visibleWindow: {
                    left: 0,
                    top: 0,
                    right: 1_000_000,
                    bottom: 1_000_000,
                  },
                }}
                selected={null}
                onSelect={() => {}}
                text={text}
              />
            </section>
          )}
        </main>
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
