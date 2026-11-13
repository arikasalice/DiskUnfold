import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function ExportReviewPresenter({
  context,
  primary,
  secondary,
  actions,
}: {
  context: ReactNode;
  primary: ReactNode;
  secondary: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <ReviewSurface
      title="내보내기"
      description="수신처와 포함 정보를 확인한 다음 파일로 저장합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="형식과 공유 범위">{primary}</ReviewSection>
        <ReviewSection title="포함·제외 미리보기">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
