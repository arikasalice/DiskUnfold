import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function GuidancePresenter({
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
      title="항목 이해와 관리"
      description="관측 근거와 불확실성을 확인하고 적절한 관리 방법을 선택합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="역할과 근거">{primary}</ReviewSection>
        <ReviewSection title="관리 방법과 영향">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
