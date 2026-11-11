import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function PlanReviewPresenter({
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
      title="정리 계획"
      description="대상과 예상 효과를 검토한 뒤 승인 검토로 넘어갑니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="계획 대상">{primary}</ReviewSection>
        <ReviewSection title="효과와 제한">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
