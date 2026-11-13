import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function ConnectionPresenter({
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
      title="AI 연결"
      description="이 기기의 연결 상태와 외부 AI에 전달할 정보 범위를 확인합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="연결과 공유 범위">{primary}</ReviewSection>
        <ReviewSection title="연결키와 호환 정보">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
