# F13 gui-web 설계 대조 감사 (PR 4: GUI 입력·접근성 공통 부품)

- 기준 커밋: `3fbdcbf7` (origin/0.0.1), 브랜치 `codex/f13-gui-input-a11y`
- 설계 근거: AIDA DiskUnfold 0.1 F13 HLD BL-HLD-1.9(rev105), DLD BL-DLD-1.5(rev82). 항목 문구는 `aida_get_design_conformance_checklist`로 읽었고, 슬롯 본문은 응답에서 제공되지 않아(`SECTION_UNAVAILABLE:item_slots`) 진술·세부 항목 문장과 DLD 문서 본문으로 대조했다.
- 판정은 사람이 검토할 소견이며 확정 판정이 아니다. 경로는 `gui-web/` 기준이다.
- `NFR2.HD1.gui-web.DD1`은 발행본에 없다. 같은 번호 계열의 `NFR2.HD1.app-host.DD1` 중 gui-web 정적 검사 부분만 마지막 행에서 본다.
- 이 감사 시점의 기준선: `npm run check` 통과, `npm run test` 665 통과·1 실패(`tests/architecture.test.ts` 20초 제한 초과; 코드가 아니라 이 PC의 프로세스 기동 지연으로 보이며, 단독 재실행은 구현 단계에서 확인한다).

## 판정 표

| DD | 판정 | 근거(파일:줄) | 비고 |
| --- | --- | --- | --- |
| FR3.HD1.gui-web.DD1 locale.changed 처리 | 일치 가능 | `src/state/use-locale.ts:191-207`(이벤트 판 비교 후 `refresh`), `:171-178`(낮은 판 응답 버림), `:67-82`(언어별 묶음 판이 바뀔 때만 `i18n.get_bundle`), `src/app/DesktopLocale.tsx:47-72`(문서 `lang` 갱신, 작업·선택 상태는 이 훅 밖에 있어 유지) | 판 번호가 없는 이벤트는 보수적으로 다시 읽는다(`:201-206`). 설계의 "버린다"와 달리 읽기 한 번이 더 생길 수 있으나 상태는 보존된다. 이 PR 범위 밖이라 수정하지 않는다. |
| FR4.HD1.gui-web.DD1 TypeScript 형식기 | 일치 가능 | `src/i18n/regional-format.ts:24-103`, `src/i18n/bytes.ts:41-67`(BigInt 정수, 지역 형식 표), `tests/locale-format-vectors.test.ts`, `tests/space-format.test.ts` | 지역 형식이 없을 때의 대체 경로가 `toLocaleString`/`Intl`을 쓴다(`regional-format.ts:25`, `bytes.ts:91-107`). Rust 형식기와 같은 규칙이 아니므로 실제 화면이 이 경로를 타는지는 별도 확인이 필요하다. 이 PR 범위 밖. |
| FR6.HD1.gui-web.DD1 문구 해석·대체 사슬·미확인 표지 | 일치 가능 | `src/state/locale-text.ts:110-128`(선택 언어 → 기준 언어 → 키, 한 문자열에 표지를 붙여 표시와 접근성 이름이 같다), `src/state/locale-missing.ts:4-45`(프레임당 한 번 `i18n.report_missing`, 실패 무시) | 오류 원인·조치 문구와 코드·사유 키 표시는 `DesktopLocale.tsx`·`LocaleSettings.tsx` 경로만 확인했고 다른 화면의 오류 상세는 확인하지 않았다. `components/data-table.tsx:45`는 접근성 이름에 영어 `scroll area`를 고정으로 붙인다(문구 키 없음, 불일치 의심). |
| FR7.HD1.gui-web.DD1 긴 문구·배율·1024×700 | **미구현 의심** | 공통 줄임 부품 없음. 한 줄 줄임이 화면별 CSS로 흩어져 있다: `theme/breadcrumbs.css:39-41`, `theme/history.css:157-158,276-277`, `theme/titlebar.css:35`. 두 줄 제한은 `theme/browse-grid.css:213-222`(행 머리글·피드백 이름)에만 있고 전체 문구 초점 설명이 없다. 가운데 줄임 없음(경로 문구는 `.du-text--path`에서 줄바꿈만). `components/data-table.tsx:3,29,32`가 열 폭 기본 160px·행 높이 144px 고정. | 주요 조작이 스크롤로 닿는다는 점은 `.du-table-viewport`(`theme/workspace.css`)·`closing.css:7` 주석 등 CSS로만 뒷받침되고 1024×700·200% 시험이 없다. happy-dom은 배치를 계산하지 않으므로 시험은 구조(스크롤 가능 영역·초점 가능·전체 문구 노출)까지만 확인할 수 있다. |
| FR8.HD1.gui-web.DD1 접근성 이름·상태 표시 표·입력 오류 | **불일치 의심** | (a) 이름: `components/controls.tsx:34-35`는 아이콘 전용 `Action`만 `aria-label`을 강제하고, 필드는 `label` 문자열을 받는다. 키가 아니라 해석된 문자열이라 "문구 키 필수"를 정적으로 검사할 수단이 없다. (b) 상태: `components/presentation.tsx:81-98` `StatusBadge`는 `tone`과 자식 문구만 받고 모양(아이콘)·문구 키·색 토큰을 한 표로 묶지 않으며, 호출부가 `tone`과 문구를 각자 고른다(`features/history/HistoryPanel.tsx:375`, `features/support/CombinationTable.tsx:41` 등). (c) 입력 오류: `controls.tsx:117-119`는 설명 칸에 `aria-live="polite"`만 있고 오류 문구를 칸에 연결(`isInvalid`/`FieldError`)하지 않으며 `ReviewField`(`components/review-field.tsx`)에는 오류 경로가 없다. (d) 정적 검사: `eslint.config.mjs`에 이름 문구 키·색만 있는 상태에 대한 규칙이 없다. | `Notice`는 `role="alert"`로 한 번 알린다(`presentation.tsx:128-146`)는 점은 일치. |
| FR8.HD2.gui-web.DD1 키 처리 순서·키 배정 표 | **미구현 의심** | 공통 키 처리기·키 배정 표 모듈 없음. 키 판정이 흩어져 있다: `components/browse-keys.ts:22-37`(조합·입력란·팝업 판정 일부), `components/browse-grid.tsx:23-34,65-75,160-200`(`gridKeys` 집합과 이동 표), `components/geometry-renderer.tsx:215-230`, `components/item-actions.tsx:50-60,84-95`, `features/browse/BrowsePanel.tsx:263-275`, `features/browse/use-browse-paging.ts:15,136-141`. 모두 `event.key`(논리 키)를 쓰고 `KeyboardEvent.code`는 쓰지 않는다. | 글자 키 논리 키 규칙은 현재 코드가 지키지만 정적 검사가 없다. 행 구조(화면 식별자·논리 키·수정 키·동작 식별자·설명 문구 키)와 같은 화면 중복 배정 검사 없음. 초점이 사라질 때 다음·이전·상위 이동은 `browse-grid.tsx`의 `focusRequest` 경로에서만 보이며 공통 규칙은 확인하지 못했다. |
| FR10.HD1.gui-web.DD1 IME 판정표 | 일치 가능(훅) / **불일치 의심(분산)** | 판정 본체: `components/ime-composition.ts:28-47`(조합 시작·갱신·끝 이벤트), `:50-69`(`isComposing`·`keyCode 229`·판정 불가), `:44-47,71-76`(조합 직후 표지를 다음 키 놓기나 다음 작업 차례에 지움). 시험: `tests/ime-input.test.tsx`가 조합 중 Enter·Esc, 조합 직후 Enter, 판정 불가 Enter, 다음 Enter 1회를 `PathField`·`ReviewField`에서 확인한다. | 판정표가 코드 흐름에 녹아 있어 7행이 표로 드러나지 않는다. 승인 입력란 Enter 미제출, 단추 Space/Enter/클릭 1회 행은 시험이 없다. 같은 판정이 따로 구현돼 있다: `browse-keys.ts:26-27`, `item-actions.tsx:53,87`, `geometry-renderer.tsx:217`, `features/browse/use-browse-paging.ts:138-139`(`isComposing`/`keyCode===229`를 각자 검사). 설계는 "판정 표는 한 곳, 화면별 재구현 없음"이다. |
| FR10.HD2.gui-web.DD1 승인 대화상자 | 일치 가능(부분) | `components/confirm-dialog.tsx:48`(취소 단추 `autoFocus`), `:35-38`(Esc·바깥 누름은 취소), `:49-55`(승인은 `Action` 누름으로만), `tests/closing-guard.test.tsx:317-336`(취소 후 초점 복귀) | 대화상자 안 입력란의 Enter 미제출 규칙은 코드에도 시험에도 없다(`ConfirmDialog`는 `children`을 그대로 받는다). 기본 초점이 취소 단추라는 단정 시험이 없다. |
| FR11.HD1.gui-web.DD1 움직임 | 불일치 의심(위치) | `src/app/DesktopLocale.tsx:47-72`가 `a11y.motion`과 `prefers-reduced-motion` 미디어 질의(`:57-62`, 변경 통지 `:65`, 질의 불가 시 꺼짐 `typeof matchMedia` 검사)를 합쳐 `data-du-motion`을 정한다. `theme/locale.css:16-26`이 줄임일 때 전환·애니메이션 시간을 0으로 만든다. 별개로 `theme/tokens.css:166-169`, `theme/workspace.css:419-423`, `theme/review-surface.css:85`가 OS 질의를 직접 읽는다. | 판정이 theme 모듈이 아니라 `app/`에 있고, 시험에서 네 조합(OS 꺼짐/켜짐 × 앱 follow_os/reduce)과 질의 불가를 확인하는 곳이 없다(`tests/locale-workspace.test.tsx:168`은 질의 스텁만). 동작 자체는 설계와 같다. |
| FR12.HD1.gui-web.DD1 배율 | 일치 가능(부분) | `src/app/DesktopLocale.tsx:48,55`가 `--du-app-scale`을 정하고 `theme/locale.css:13`의 `zoom: var(--du-app-scale, 1)` 한 곳에서 적용한다. 값 집합은 `ipc/generated/contracts.ts:471`(`100|110|125|150|175|200`). | 판정·허용 값 확인이 theme에 없고, 시험은 배율 값별 토큰 적용을 보지 않는다. 지정되지 않은 값이 들어올 때의 동작이 없다. |
| NFR2.HD1 (gui-web 정적 검사 부분) | 일치 가능 | `scripts/check-locale.mjs:82-130`가 사용 키(`ui.*` 문자열)와 선언 목록 `message-keys.txt`, 영어·한국어 묶음을 대조해 미선언 사용·누락·매개변수 차이는 실패, 여분 키는 경고로 처리한다. 현재 1089 선언·853 사용에서 통과. | "메타 없는 사유 키"는 이 스크립트가 아니라 arch-check 쪽 항목이다. 이 PR 소유 경로 밖이라 변경하지 않는다. |

## 이 PR에서 메우는 항목

1. FR10.HD1: IME 판정을 `ime-composition.ts`의 판정 표 하나로 만들고 흩어진 `isComposing`/`keyCode===229` 검사를 그 호출로 바꾼다. 7행 각각 시험.
2. FR10.HD2: `ConfirmDialog` 안 입력란의 Enter 미제출·기본 초점 취소 시험과 방어 구현.
3. FR8.HD2: `key-dispatch.ts`(조합 → 입력란 → 대화상자 → 키 배정 표 → 무시)와 `key-table.ts`(행 구조, 중복 검사). 탐색 격자 키 집합을 표로 옮긴다.
4. FR8.HD1: `status-table.ts`와 `StatusBadge` 연결, 입력 오류의 칸 연결, ESLint 규칙 세 가지(이름 없는 조작 부품, 색만 있는 상태 행, `KeyboardEvent.code` 배정 금지)와 위반 fixture.
5. FR7.HD1: 끝 줄임·가운데 줄임 부품과 시험.
6. FR11/FR12: theme 모듈에 움직임 수준·배율 판정을 두고 네 조합·여섯 배율 시험(`app/DesktopLocale.tsx`의 호출 교체는 소유 경로 밖이므로 보고만 한다).

## 소유 경로 밖이라 보고만 하는 항목

- `src/app/DesktopLocale.tsx`: 움직임·배율 판정 호출을 theme 모듈 호출로 교체(작은 변경).
- 새 사용자 문구가 필요할 때 `user-env/i18n/en.json`·`ko.json`·`message-keys.txt` 추가(구현 단계에서 목록으로 보고).
- `features/browse/BrowsePanel.tsx:263-275`의 `Alt+ArrowUp` 처리를 키 배정 표로 옮기는 변경.
