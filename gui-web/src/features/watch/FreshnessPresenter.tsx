import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function FreshnessPresenter({
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
      title="변경 감지와 갱신"
      description="마지막 관측과 감시 상태를 구분해 필요한 범위를 다시 확인합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="감시 상태">{primary}</ReviewSection>
        <ReviewSection title="관측 갱신">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
