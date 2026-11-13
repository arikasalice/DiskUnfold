import {
  Action,
  DetailDisclosure,
  ItemActions,
  Notice,
  Readout,
  ReviewCheck,
  ReviewField,
  StatusBadge,
  Text,
} from "../src/components";
import type { SurfaceId } from "./surface-fixtures";
import { fixtures } from "./surface-fixtures";
import { recordActions } from "./record-actions";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { fixtureDisplayText, modelFixture } from "./model-fixture";

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
            actions={recordActions(surface, name, open)}
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
  const { frame, page } = modelFixture();
  const index = rows.findIndex((row) => row[0] === selected);
  const shape = frame.model.shapes[index];
  return (
    <div className="du-review-chart">
      <VisualizationModelPresenter
        frame={frame}
        page={page}
        text={fixtureDisplayText}
        view={{
          viewport: { width: 960, height: 330, scale: 1 },
          visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
        }}
        selected={
          shape?.itemRef
            ? {
                kind: "item",
                itemRef: shape.itemRef,
                matchKey: shape.matchKey,
                modelRef: frame.model.modelRef,
                resultRevision: frame.resultRevision,
              }
            : null
        }
        onSelect={(selection) => {
          const row =
            selection.kind === "item"
              ? rows[selection.itemRef.itemNo - 1]
              : undefined;
          if (row) onSelect({ name: row[0], value: row[1], detail: row[2] });
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
