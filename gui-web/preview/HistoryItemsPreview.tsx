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
 * Below it, the count sentences and empty states of #204 on their own records,
 * then the same shared path text on its own, outside a table.
 */
export function HistoryItemsPreview() {
  return (
    <LocaleTextContext.Provider value={koreanText}>
      <ItemRecord jobRef="preview-job" label="작업 상세" />
      <ItemRecord jobRef="preview-partial" label="일부만 남은 기록" />
      <ItemRecord
        jobRef="preview-total-unknown"
        label="전체 수를 모르는 기록"
      />
      <ItemRecord
        jobRef="preview-not-recorded"
        label="항목별 결과를 남기지 않은 기록"
      />
      <ItemRecord
        jobRef="preview-none-processed"
        label="처리한 항목이 없는 기록"
      />
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

/** One recorded operation's item results in the history detail frame. */
function ItemRecord({ jobRef, label }: { jobRef: string; label: string }) {
  return (
    <section className="du-history-screen" aria-label={label}>
      <Heading level={3}>{label}</Heading>
      <div className="du-history-detail">
        <DetailDisclosure
          title={koreanText("ui.history.item_results", "Item results")}
          defaultExpanded
        >
          <HistoryItems client={historyClient} jobRef={jobRef} />
        </DetailDisclosure>
      </div>
    </section>
  );
}
