import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function PreferencesPresenter({
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
      title="설정"
      description="설정을 찾고 적용 시점과 영향을 확인합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="설정 검색과 편집">{primary}</ReviewSection>
        <ReviewSection title="변경 검토">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
