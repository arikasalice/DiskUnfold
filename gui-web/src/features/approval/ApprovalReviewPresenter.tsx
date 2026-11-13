import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function ApprovalReviewPresenter({
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
      title="변경 요청 검토"
      description="검토한 대상과 작업에 대해서만 승인합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="작업·목적지·대상">{primary}</ReviewSection>
        <ReviewSection title="영향·보호·복원 제한">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
