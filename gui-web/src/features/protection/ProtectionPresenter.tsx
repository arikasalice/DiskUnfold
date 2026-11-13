import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function ProtectionPresenter({
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
      title="보관과 보호"
      description="분석 제외와 변경 금지의 적용 범위를 구분합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="현재 보호 규칙">{primary}</ReviewSection>
        <ReviewSection title="변경 미리보기">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
