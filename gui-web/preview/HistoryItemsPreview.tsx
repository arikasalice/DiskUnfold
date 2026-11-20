import {
  DetailDisclosure,
  Heading,
  MiddleEllipsisText,
  Text,
} from "../src/components";
import { HistoryItems } from "../src/features/history/HistoryItems";
import { LocaleTextContext } from "../src/i18n/presentation";
import "../src/theme/history.css";
import { historyClient, standaloneLocation } from "./history-items-fixture";
import { koreanText } from "./result-summary-fixture";

/**
 * The product's item results of the operation history on a fixed record, in
 * the detail frame the history screen gives them, in Korean (F02 README #170).
 * Below it, the same shared path text on its own, outside a table.
 */
export function HistoryItemsPreview() {
  return (
    <LocaleTextContext.Provider value={koreanText}>
      <section className="du-history-screen" aria-label="작업 상세">
        <div className="du-history-detail">
          <DetailDisclosure
            title={koreanText("ui.history.item_results", "Item results")}
            defaultExpanded
          >
            <HistoryItems client={historyClient} jobRef="preview-job" />
          </DetailDisclosure>
        </div>
      </section>
      <section className="du-gallery-standalone" aria-label="표 밖 단독 값">
        <Heading level={3}>표 밖 단독 값</Heading>
        <Text kind="muted">
          같은 공용 부품입니다. 마지막 마디가 줄보다 길면 확장자를 남기고 그
          가운데를 줄입니다.
        </Text>
        <div className="du-gallery-standalone__value">
          <MiddleEllipsisText text={standaloneLocation} />
        </div>
      </section>
    </LocaleTextContext.Provider>
  );
}
