import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function ComparisonPresenter({
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
      title="변화 비교"
      description="같은 기준에서 비교할 수 있는 변화와 확인할 수 없는 범위를 구분합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-stack">
        <ReviewSection title="비교 기준">{primary}</ReviewSection>
        <ReviewSection title="변화와 근거">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
