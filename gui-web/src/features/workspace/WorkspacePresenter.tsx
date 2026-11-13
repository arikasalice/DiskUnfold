import type { ReactNode } from "react";
import { ReviewSection, ReviewSurface } from "../../components";

/** Presentation only; the owning controller supplies published data and actions. */
export function WorkspacePresenter({
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
      title="공간 탐색"
      description="공간 분포를 먼저 확인하고 필요한 항목을 자세히 조사합니다."
      context={context}
      tools={actions}
    >
      <div className="du-review-stack">
        <ReviewSection title="분포 보기">{primary}</ReviewSection>
        <ReviewSection title="항목 목록">{secondary}</ReviewSection>
      </div>
    </ReviewSurface>
  );
}
