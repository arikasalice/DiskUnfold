import { useState } from "react";
import { Action, Text } from "../src/components";
import type { ScanResourceMode } from "../src/ipc";
import { LocaleTextContext } from "../src/i18n/presentation";
import { ScopeCollectionConditions } from "../src/features/analysis/ResourceControls";
import type { ResourceLimitError } from "../src/features/analysis/ResourceControls";
import { koreanText } from "./result-summary-fixture";
import "./collection-conditions-preview.css";

export type CollectionPreviewScene =
  | "scope-collapsed"
  | "scope-expanded"
  | "scope-error-known"
  | "scope-error-unknown";

const sceneValues: Record<
  CollectionPreviewScene,
  {
    mode: ScanResourceMode;
    userLimit: number | undefined;
    draft: string;
    error: ResourceLimitError | undefined;
    previous: number | undefined;
    max: number | undefined;
    expanded: boolean;
    advancedExpanded: boolean;
  }
> = {
  "scope-collapsed": {
    mode: "auto",
    userLimit: undefined,
    draft: "",
    error: undefined,
    previous: undefined,
    max: undefined,
    expanded: false,
    advancedExpanded: false,
  },
  "scope-expanded": {
    mode: "fast",
    userLimit: 8,
    draft: "8",
    error: undefined,
    previous: undefined,
    max: undefined,
    expanded: true,
    advancedExpanded: true,
  },
  "scope-error-known": {
    mode: "fast",
    userLimit: 12,
    draft: "12",
    error: "invalid",
    previous: 4,
    max: 6,
    expanded: true,
    advancedExpanded: true,
  },
  "scope-error-unknown": {
    mode: "quiet",
    userLimit: 3,
    draft: "3.5",
    error: "invalid",
    previous: 3,
    max: undefined,
    expanded: true,
    advancedExpanded: true,
  },
};

/** Static scope-review examples for the collection-condition gallery. */
export function CollectionConditionsPreview({
  scene,
}: {
  scene: CollectionPreviewScene;
}) {
  const initial = sceneValues[scene];
  const [mode, setMode] = useState(initial.mode);
  const [userLimit, setUserLimit] = useState(initial.userLimit);
  const [draft, setDraft] = useState(initial.draft);
  const [error, setError] = useState(initial.error);
  const [previous, setPrevious] = useState(initial.previous);

  return (
    <LocaleTextContext.Provider value={koreanText}>
      <main className="du-scope-preview">
        <header className="du-scope-preview__header">
          <Text kind="muted">DiskUnfold / 내 작업공간</Text>
          <h1>분석 범위 확인</h1>
          <p>포함할 위치와 제외 규칙을 확인한 뒤 시작합니다.</p>
        </header>
        <div className="du-scope-preview__grid">
          <section className="du-scope-preview__card">
            <h2>포함 위치</h2>
            <div className="du-scope-preview__scope-row">
              <span>선택됨</span>
              <span>C:\Users\민서\Documents</span>
              <span>접근 가능</span>
            </div>
            <div className="du-scope-preview__scope-row">
              <span>추가</span>
              <span>폴더 또는 경로 추가</span>
              <span>…</span>
            </div>
            <h2>제외 규칙</h2>
            <div className="du-scope-preview__scope-row">
              <span>이름 패턴</span>
              <span>*.tmp</span>
              <span>유효</span>
            </div>
            <Text kind="muted">
              겹치는 포함 경로는 한 분석 경로로 합쳐집니다.
            </Text>
          </section>
          <aside className="du-scope-preview__card du-scope-preview__summary">
            <h2>분석 요약</h2>
            <p>포함 위치 1개 · 제외 규칙 2개 · 메타데이터 분석</p>
            <div className="du-scope-preview__actions">
              <Action variant="primary">분석 시작</Action>
              <Action>취소</Action>
            </div>
            <ScopeCollectionConditions
              mode={mode}
              userLimit={userLimit}
              draft={draft}
              error={error}
              previousValue={error ? previous : userLimit}
              max={initial.max}
              disabled={false}
              defaultExpanded={initial.expanded}
              advancedExpanded={initial.advancedExpanded}
              onModeChange={setMode}
              onLimitChange={(value) => {
                setDraft(value);
                const parsed = Number(value);
                if (value === "") {
                  setUserLimit(undefined);
                  setError(undefined);
                } else if (
                  Number.isSafeInteger(parsed) &&
                  parsed >= 1 &&
                  (initial.max === undefined || parsed <= initial.max)
                ) {
                  setUserLimit(parsed);
                  setError(undefined);
                } else {
                  setError("invalid");
                  setPrevious(userLimit);
                }
              }}
              onReset={() => {
                setMode("auto");
                setUserLimit(undefined);
                setDraft("");
                setError(undefined);
                setPrevious(undefined);
              }}
            />
          </aside>
        </div>
      </main>
    </LocaleTextContext.Provider>
  );
}
