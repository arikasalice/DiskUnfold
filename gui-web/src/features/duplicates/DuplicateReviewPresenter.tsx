import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function DuplicateReviewPresenter({
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
      title="중복 검토"
      description="검증된 묶음과 아직 확인 중인 후보를 구분해 확인합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="검증된 묶음">{primary}</ReviewSection>
        <ReviewSection title="확인 중인 후보">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
