import {
  Action,
  DetailDisclosure,
  GeometryRenderer,
  ItemActions,
  Notice,
  Readout,
  ReviewCheck,
  ReviewField,
  StatusBadge,
  Text,
} from "../src/components";
import type { SurfaceId } from "./surface-fixtures";
import type { VizGeometry } from "../src/ipc";
import { fixtures } from "./surface-fixtures";

export type SelectedRecord = { name: string; value: string; detail: string };

export function FixtureRecords({
  surface,
  query,
  onSelect,
}: {
  surface: SurfaceId;
  query: string;
  onSelect: (record: SelectedRecord) => void;
}) {
  const rows = fixtures[surface].rows.filter((row) =>
    row.join(" ").toLocaleLowerCase().includes(query.toLocaleLowerCase()),
  );
  return (
    <div>
      {rows.length === 0 && <Text>조건에 맞는 샘플 항목이 없습니다.</Text>}
      {rows.map(([name, value, detail]) => {
        const open = () => onSelect({ name, value, detail });
        return (
          <ItemActions
            key={name}
            label={`${name} 동작`}
            actions={[
              { id: "details", label: "자세히 보기", onAction: open },
              {
                id: "location",
                label: "파일 위치 열기",
                disabledReason: "실제 대상 연결 전",
                onAction: () => {},
              },
              {
                id: "plan",
                label: "정리 계획에서 검토",
                disabledReason: "실제 대상 검증과 계획 연결 전",
                onAction: () => {},
              },
            ]}
          >
            <div>
              <Action variant="quiet" onPress={open}>
                {name}
              </Action>
              <Text>{value}</Text>
              <Text kind="muted">{detail}</Text>
            </div>
          </ItemActions>
        );
      })}
    </div>
  );
}

export function FixtureChart({
  selected,
  onSelect,
}: {
  selected: string | null;
  onSelect: (record: SelectedRecord) => void;
}) {
  const rows = fixtures.workspace.rows;
  const points = [
    [
      { x: 0, y: 0 },
      { x: 500000, y: 1000000 },
    ],
    [
      { x: 504000, y: 0 },
      { x: 1000000, y: 500000 },
    ],
    [
      { x: 504000, y: 510000 },
      { x: 834000, y: 1000000 },
    ],
    [
      { x: 838000, y: 510000 },
      { x: 1000000, y: 1000000 },
    ],
  ];
  const geometry: VizGeometry[] = points.map((points) => ({
    primitive: "rect",
    points,
  }));
  return (
    <div className="du-review-chart">
      <GeometryRenderer
        modelRef="synthetic-layout"
        resultRevision="synthetic-1"
        label="샘플 공간 분포"
        selectedText="선택됨"
        fallbackNotice="도형 대신 같은 샘플의 목록을 표시합니다."
        missingFallbackNotice="같은 샘플 목록이 준비되지 않았습니다."
        width={960}
        height={330}
        viewport={{ left: 0, top: 0, right: 1000000, bottom: 1000000 }}
        shapes={rows.map(([name, value], index) => ({
          shapeRef: `shape-${index}`,
          rowRef: name,
          geometry: geometry[index]!,
          accessibleLabel: `${name} · ${value}`,
        }))}
        relations={[]}
        selectedRowRef={selected}
        onSelect={(name) => {
          const row = rows.find((row) => row[0] === name);
          if (row) onSelect({ name: row[0], value: row[1], detail: row[2] });
        }}
        fallback={{
          modelRef: "synthetic-layout",
          resultRevision: "synthetic-1",
          content: (
            <DetailDisclosure title="같은 분포를 목록으로 보기">
              <Readout
                label="샘플 분포 수치"
                rows={rows.map(([label, value]) => ({ label, value }))}
              />
            </DetailDisclosure>
          ),
        }}
      />
    </div>
  );
}

export function FixtureDetails({
  surface,
  checked,
  setChecked,
  draft,
  setDraft,
}: {
  surface: SurfaceId;
  checked: boolean;
  setChecked: (value: boolean) => void;
  draft: string;
  setDraft: (value: string) => void;
}) {
  return (
    <>
      <Readout
        label="검토 맥락"
        rows={fixtures[surface].facts.map(([label, value]) => ({
          label,
          value,
        }))}
      />
      <Notice>{fixtures[surface].notice}</Notice>
      {surface === "approval" && (
        <>
          <ReviewCheck
            label="표시된 대상과 작업을 확인했습니다 (UI 동작 예시)"
            checked={checked}
            onChange={setChecked}
          />
          <Action isDisabled>승인 불가 — 실제 요청 없음</Action>
        </>
      )}
      {surface === "settings" && (
        <>
          <ReviewField
            label="설정 검색어 예시"
            value={draft}
            onChange={setDraft}
            description="입력은 이 갤러리의 메모리에만 남습니다."
          />
          <StatusBadge>
            {draft ? "편집 중 · 적용되지 않음" : "변경 없음"}
          </StatusBadge>
        </>
      )}
      {surface === "duplicates" && (
        <Text>
          후보 1개 묶음 · 검증되지 않음 · 어떤 사본도 선택하지 않았습니다.
        </Text>
      )}
      <DetailDisclosure title="제한과 연결 상태">
        <Text>
          샘플 표시 전용입니다. 데이터 조회·저장·파일 변경 콜백은 연결되지
          않았습니다.
        </Text>
        <Text>
          확인 불가·미기록·적용 불가·진행 대기는 서로 다른 상태이며, 근거 없이
          바꾸어 표시하지 않습니다.
        </Text>
      </DetailDisclosure>
    </>
  );
}
