import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function ScenarioPresenter({
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
      title="가정 비교"
      description="실측 기준을 유지하며 가정의 효과와 제한을 비교합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-stack">
        <ReviewSection title="기준과 가정">{primary}</ReviewSection>
        <ReviewSection title="반영 결과와 충돌">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
