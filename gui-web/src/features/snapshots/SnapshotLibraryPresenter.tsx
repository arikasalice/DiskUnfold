import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function SnapshotLibraryPresenter({
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
      title="저장된 관측"
      description="관측 시점과 범위를 확인한 뒤 저장된 결과를 엽니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-columns">
        <ReviewSection title="스냅샷과 저장소">{primary}</ReviewSection>
        <ReviewSection title="선택한 관측">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
