import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function JobCenterPresenter({
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
      title="작업 현황"
      description="요청한 제어와 실제 작업 상태를 구분해 확인합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="진행 중인 작업">{primary}</ReviewSection>
        <ReviewSection title="완료와 항목별 결과">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
