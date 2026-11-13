import type { ItemAction } from "../src/components";
import type { SurfaceId } from "./surface-fixtures";

type PlannedAction = readonly [id: string, label: string, reason: string];

// These are review candidates, not a capability declaration from the engine.
const plannedActions: Record<SurfaceId, readonly PlannedAction[]> = {
  workspace: [
    ["location", "파일 위치 열기", "실제 대상 연결 전"],
    ["plan", "정리 계획에서 검토", "현재 대상 검증과 계획 연결 전"],
  ],
  snapshots: [
    ["open", "관측 결과 열기", "저장된 결과 연결 전"],
    ["compare", "다른 관측과 비교", "비교 기준과 호환성 확인 전"],
  ],
  comparison: [
    ["evidence", "양쪽 관측에서 확인", "비교 근거 연결 전"],
    ["export", "변화 보고서 검토", "보고서와 공유 범위 연결 전"],
  ],
  duplicates: [
    ["copies", "사본별 위치 확인", "실제 대상 연결 전"],
    ["plan", "정리 계획에서 검토", "사본 선택과 보관 조건 확인 전"],
  ],
  planning: [
    ["edit", "계획 항목 편집", "불변 계획의 새 판 생성 연결 전"],
    ["exclude", "계획에서 제외", "계획 편집 연결 전 · 파일을 삭제하지 않음"],
  ],
  approval: [["evidence", "대상 확인 근거 보기", "실제 승인 요청 연결 전"]],
  jobs: [
    ["pause", "일시정지 요청", "실제 작업 상태와 지원 여부 연결 전"],
    ["resume", "계속 진행 요청", "실제 일시정지 상태 확인 전"],
    ["cancel", "취소 요청", "실제 작업 연결 전 · 완료된 변경은 되돌리지 않음"],
  ],
  protection: [
    ["edit", "규칙 편집 검토", "규칙 소유자와 변경 영향 확인 전"],
    ["scope", "적용 범위 확인", "실제 규칙과 대상 연결 전"],
  ],
  export: [["inclusion", "포함 기준 검토", "공유 정책과 미리보기 연결 전"]],
  settings: [
    ["edit", "설정값 편집", "설정 스키마와 적용 시점 연결 전"],
    ["reset", "기본값 복원 검토", "기본값과 변경 범위 확인 전"],
  ],
  connections: [
    ["guide", "연결 방법 보기", "호환 정보 연결 전"],
    ["sharing", "공유 범위 검토", "공유 정책 연결 전"],
  ],
  guidance: [
    ["evidence", "판단 근거 확인", "항목별 규칙 근거 연결 전"],
    ["method", "관리 방법 검토", "실제 항목과 관리 방법 연결 전"],
  ],
  watch: [
    ["refresh", "다시 분석", "실제 분석 범위 연결 전"],
    ["scope", "감시 범위 검토", "지원 환경과 감시 설정 연결 전"],
  ],
  scenario: [
    ["edit", "가정 편집", "기준 관측과 가정 연결 전"],
    ["evidence", "반영 근거와 충돌 확인", "평가 결과 연결 전"],
  ],
  history: [["result", "연결된 결과 열기", "기록에 연결된 결과 확인 전"]],
};

export function recordActions(
  surface: SurfaceId,
  name: string,
  openDetails: () => void,
): ItemAction[] {
  const readOnly =
    (surface === "settings" && name === "앱 정보") ||
    (surface === "jobs" && name === "이전 정리 작업");
  return [
    { id: "details", label: "자세히 보기", onAction: openDetails },
    ...(readOnly ? [] : plannedActions[surface]).map(
      ([id, label, disabledReason]) => ({
        id,
        label,
        disabledReason,
        onAction: () => {},
      }),
    ),
  ];
}
