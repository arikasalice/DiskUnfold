import { useState } from "react";
import {
  Action,
  ContentState,
  DetailDisclosure,
  Heading,
  InspectionDialog,
  LiveStatus,
  Notice,
  Readout,
  ReviewField,
  ReviewSurface,
  SelectField,
  StatusBadge,
  Text,
} from "../src/components";
import type { SurfaceState } from "../src/components";
import { WorkspacePresenter } from "../src/features/workspace/WorkspacePresenter";
import { SnapshotLibraryPresenter } from "../src/features/snapshots/SnapshotLibraryPresenter";
import { ComparisonPresenter } from "../src/features/comparison/ComparisonPresenter";
import { DuplicateReviewPresenter } from "../src/features/duplicates/DuplicateReviewPresenter";
import { PlanReviewPresenter } from "../src/features/planning/PlanReviewPresenter";
import { ApprovalReviewPresenter } from "../src/features/approval/ApprovalReviewPresenter";
import { JobCenterPresenter } from "../src/features/jobs/JobCenterPresenter";
import { ProtectionPresenter } from "../src/features/protection/ProtectionPresenter";
import { ExportReviewPresenter } from "../src/features/export/ExportReviewPresenter";
import { PreferencesPresenter } from "../src/features/settings/PreferencesPresenter";
import { ConnectionPresenter } from "../src/features/connections/ConnectionPresenter";
import { GuidancePresenter } from "../src/features/guidance/GuidancePresenter";
import { FreshnessPresenter } from "../src/features/watch/FreshnessPresenter";
import { ScenarioPresenter } from "../src/features/scenario/ScenarioPresenter";
import coverage from "./feature-coverage.json";
import { fixtures } from "./surface-fixtures";
import type { SurfaceId } from "./surface-fixtures";
import { FixtureChart, FixtureDetails, FixtureRecords } from "./GalleryContent";
import type { SelectedRecord } from "./GalleryContent";

const presenters = {
  workspace: WorkspacePresenter,
  snapshots: SnapshotLibraryPresenter,
  comparison: ComparisonPresenter,
  duplicates: DuplicateReviewPresenter,
  planning: PlanReviewPresenter,
  approval: ApprovalReviewPresenter,
  jobs: JobCenterPresenter,
  protection: ProtectionPresenter,
  export: ExportReviewPresenter,
  settings: PreferencesPresenter,
  connections: ConnectionPresenter,
  guidance: GuidancePresenter,
  watch: FreshnessPresenter,
  scenario: ScenarioPresenter,
};
const messages: Record<SurfaceState, string> = {
  ready: "",
  loading: "샘플 화면을 읽는 중입니다. 총량은 아직 알 수 없습니다.",
  empty: "표시할 샘플 결과가 없습니다. 범위와 조건을 확인하세요.",
  error: "샘플 조회가 실패했습니다. 입력 조건은 그대로 유지합니다.",
  partial: "일부 결과만 확인되었습니다. 누락 범위를 함께 확인하세요.",
  stale: "이전 관측을 표시하고 있습니다. 현재 상태는 다시 확인해야 합니다.",
  unavailable: "이 환경에서 사용할 수 있는 결과가 없습니다. 이유를 확인하세요.",
};
const stateOptions: { value: SurfaceState; label: string }[] = [
  { value: "ready", label: "결과 있음" },
  { value: "loading", label: "불러오는 중" },
  { value: "empty", label: "결과 없음" },
  { value: "error", label: "조회 실패" },
  { value: "partial", label: "부분 결과" },
  { value: "stale", label: "오래된 결과" },
  { value: "unavailable", label: "사용 불가" },
];

/** Development entry only: all data, mutations and recovery are synthetic. */
export function Gallery() {
  const [surface, setSurface] = useState<SurfaceId>("workspace");
  const [state, setState] = useState<SurfaceState>("ready");
  const [theme, setTheme] = useState("system");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<SelectedRecord>();
  const [activeShape, setActiveShape] = useState<SelectedRecord>();
  const [checked, setChecked] = useState(false);
  const [draft, setDraft] = useState("");
  const [feedback, setFeedback] = useState("");
  const fixture = fixtures[surface];
  const selectSurface = (value: SurfaceId) => {
    setSurface(value);
    setQuery("");
    setSelected(undefined);
    setActiveShape(undefined);
    setChecked(false);
    setDraft("");
    setFeedback("");
  };
  const context = (
    <>
      <StatusBadge>UI 검토용 샘플</StatusBadge>
      <Text kind="muted">실제 파일·작업 연결 없음</Text>
    </>
  );
  const records = (
    <FixtureRecords surface={surface} query={query} onSelect={setSelected} />
  );
  const details = (
    <FixtureDetails
      surface={surface}
      checked={checked}
      setChecked={setChecked}
      draft={draft}
      setDraft={setDraft}
    />
  );
  const primary = (
    <ContentState
      state={state}
      message={messages[state]}
      recovery={{
        label: "샘플 상태 복구",
        onPress: () => {
          setState("ready");
          setFeedback(
            "샘플 화면을 복구했습니다. 실제 조회는 수행하지 않았습니다.",
          );
        },
      }}
    >
      {surface === "workspace" ? (
        <>
          <FixtureChart
            selected={activeShape?.name ?? null}
            onSelect={setActiveShape}
          />
          {activeShape && (
            <div className="du-review-actions">
              <Text>
                {activeShape.name} · {activeShape.value}
              </Text>
              <Action onPress={() => setSelected(activeShape)}>
                선택 항목 자세히 보기
              </Action>
            </div>
          )}
        </>
      ) : (
        records
      )}
    </ContentState>
  );
  const secondary = ["ready", "partial", "stale"].includes(state) ? (
    surface === "workspace" ? (
      records
    ) : (
      details
    )
  ) : (
    <Text>같은 결과의 상세 정보가 준비되면 표시합니다.</Text>
  );
  const tools = (
    <Action
      onPress={() =>
        setFeedback(
          "이 화면은 레이아웃 검토 단계입니다. 실제 기능의 생성 타입과 제어기 연결이 필요합니다.",
        )
      }
    >
      연결 상태 확인
    </Action>
  );
  const props = { context, primary, secondary, actions: tools };
  const Presenter = surface === "history" ? null : presenters[surface];
  return (
    <main className="du-gallery">
      <header className="du-gallery-notice">
        <Heading level={1}>DiskUnfold UI 갤러리</Heading>
        <StatusBadge tone="warning">개발용 · 제품 기능 아님</StatusBadge>
      </header>
      <div className="du-gallery-bar">
        <SelectField
          label="검토할 화면"
          value={surface}
          options={(Object.keys(fixtures) as SurfaceId[]).map((value) => ({
            value,
            label: fixtures[value].title,
          }))}
          onChange={selectSurface}
        />
        <SelectField
          label="화면 상태"
          value={state}
          options={stateOptions}
          onChange={setState}
        />
        <SelectField
          label="검수 테마"
          value={theme}
          options={[
            { value: "system", label: "시스템" },
            { value: "light", label: "밝게" },
            { value: "dark", label: "어둡게" },
          ]}
          onChange={(value) => {
            setTheme(value);
            if (value === "system")
              delete document.documentElement.dataset.theme;
            else document.documentElement.dataset.theme = value;
          }}
        />
      </div>
      <ReviewField
        label={fixture.query}
        value={query}
        onChange={setQuery}
        description={fixture.description}
      />
      {Presenter ? (
        <Presenter {...props} />
      ) : (
        <ReviewSurface
          title={fixture.title}
          description={fixture.description}
          context={context}
        >
          {primary}
          {secondary}
        </ReviewSurface>
      )}
      <LiveStatus>{feedback}</LiveStatus>
      <DetailDisclosure title="45개 기능의 화면 배치와 연동 상태">
        <Notice>
          기능 배치는 모두 등록했습니다. 화면 틀 준비는 기능 구현·설계 적합성
          검증 완료를 뜻하지 않습니다.
        </Notice>
        <div className="du-gallery-catalog">
          {coverage.map((item) => (
            <Readout
              key={item.feature}
              label={item.feature + " " + item.title}
              rows={[
                {
                  label: item.feature + " " + item.title,
                  value: fixtures[item.primarySurface as SurfaceId].title,
                },
                {
                  label: "현재 범위",
                  value:
                    item.delivery === "existing-partial-ui"
                      ? "기존 일부 UI + 확장 화면 검토"
                      : "화면 틀 · 실제 연동 전",
                },
                {
                  label: "설계 확인",
                  value:
                    item.sourceDldRevision === null
                      ? "발행 DLD 읽기 확인 필요"
                      : "발행 DLD " + item.sourceDldRevision,
                },
              ]}
            />
          ))}
        </div>
      </DetailDisclosure>
      {selected && (
        <InspectionDialog
          title={selected.name}
          onClose={() => setSelected(undefined)}
        >
          <StatusBadge>UI 검토용 샘플</StatusBadge>
          <Readout
            label="선택 항목"
            rows={[
              { label: "이름", value: selected.name },
              { label: "표시 값", value: selected.value },
              { label: "관측·제한", value: selected.detail },
            ]}
          />
          <Text>
            상세를 닫으면 열었던 항목으로 돌아갑니다. 파일 변경은 수행하지
            않습니다.
          </Text>
          <DetailDisclosure title="원시 값과 추가 근거">
            <Text>
              이 샘플은 표시 문자열만 제공합니다. 실제 연결 시 같은 결과 판의
              원시 값·시점·근거를 표시해야 합니다.
            </Text>
          </DetailDisclosure>
        </InspectionDialog>
      )}
    </main>
  );
}
