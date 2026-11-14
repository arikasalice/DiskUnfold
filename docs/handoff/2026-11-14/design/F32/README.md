# F32 샘플 데이터 체험 · 디자인 자료 (2026-11-14)

Penpot 파일 `e280b017-e522-80cb-8008-e0ee2a1f5e75`의 보드를 내보내기 API로 1배율 PNG로 받았다. 그림은 편집하지 않았다.

**한계부터 밝힌다.** Penpot에는 이 기능의 화면 보드가 테마별로 한 장씩만 있다. 상태별 보드(불러오는 중, 빈 결과, 오류 등)는 그려져 있지 않다. 아래 "상태와 배치" 절은 발행 HLD의 화면 계약과 Penpot의 UX 계약 문구, 기존 공통 컴포넌트를 근거로 디자인팀이 글로 정한 것이며, 그림으로 검증된 내용이 아니다. 보드의 수치와 이름은 예시 데이터다. 왼쪽 탐색 메뉴가 있는 1440×960 셸은 현재 제품 셸과 다르므로 본문 영역의 구성만 기준으로 삼는다.

## 파일

| 내용 | PNG | Penpot |
| --- | --- | --- |
| sample 화면 | [Light](sample-light.png) · [Dark](sample-dark.png) | [Light](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961e9d5a&board-id=b330ca3c-df1a-80f0-8008-e10f4cec80ca) · [Dark](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961f2e17&board-id=b330ca3c-df1a-80f0-8008-e10f69a2d650) |
| UX 계약 02 · 첫 실행과 분석 (F32 포함) | [PNG](ux-contract-02-first-run-F32.png) | [Penpot](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961fff7f&board-id=b330ca3c-df1a-80f0-8008-e10e7fce9fa7) |
| 공통 컴포넌트 | [Light](components-common-light.png) · [Dark](components-common-dark.png) | [Penpot Light](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961f5601&board-id=b330ca3c-df1a-80f0-8008-e10f064233bc) · [Dark](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961f5601&board-id=b330ca3c-df1a-80f0-8008-e10f07beb07b) |
| 확인 대화상자 예시 | [Light](dialog-confirm-light.png) · [Dark](dialog-confirm-dark.png) | [Penpot Light](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961e9d5a&board-id=b330ca3c-df1a-80f0-8008-e10f4b84b94e) · [Dark](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961f2e17&board-id=b330ca3c-df1a-80f0-8008-e10f683db782) |
| UX 계약 11 · 공통 상태·접근성·모션 | [PNG](ux-contract-11-common-states.png) | [Penpot](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961fff7f&board-id=b330ca3c-df1a-80f0-8008-e10e86b29e39) |

## 보드에 그려진 것

sample 보드는 탐색 결과 화면에 체험 표시를 더한 한 장이다.

- 오른쪽 위에 "샘플 닫기" 단추가 있다.
- 본문 맨 아래, 바닥글 바로 위에 "샘플 체험 · 실제 파일과 분리됨 · 변경 실행 불가" 문구가 한 줄 있다. 강조색 글자다.
- 그 사이는 실제 탐색 화면과 같은 구성이다(검색·필터 줄, 집계 범위 줄, 트리맵, 요약, 항목 목록).

UX 계약 02에서 F32에 해당하는 문장은 두 개다. "샘플 체험은 별도 원천 표지를 유지하며 실제 파일 변경 버튼을 비활성화한다." "샘플에서 실제 분석으로 전환 시 범위를 새로 검토."

요청하신 여섯 가지 중 보드에 있는 것은 체험 표지와 탐색 문맥, 종료 단추뿐이다. 체험 시작 진입점, 준비 중 진행, 체험 불가 안내, 종료·초기화 확인, 실제 분석 전환은 그림이 없다.

## 상태와 배치 (글로 정함)

| 상태 | 배치와 문구 | 공통 컴포넌트 |
| --- | --- | --- |
| 진입점 | "새 분석" 화면의 범위 입력 아래에 보조 단추 "샘플로 먼저 체험" / "Try with sample data". 기본 동작(스캔 시작)보다 약하게 둔다. | `Action`(secondary) |
| 준비 중 | 단추가 눌린 자리에서 바로 진행 표시. 총량을 모르면 불확정 막대. 취소할 수 있으면 "체험 취소". | `Progress`, `LiveStatus`, `Action` |
| 체험 불가 | 진입점 자리에 사유와 함께 안내. 실제 분석은 계속 쓸 수 있다고 밝힌다. | `Notice`(info), 실패는 `Notice`(error) |
| 체험 중 표지 | 보드에는 맨 아래 한 줄이지만, 스크롤해도 보이도록 **본문 맨 위의 지속 띠**로 올린다. 문구는 보드 그대로 "샘플 체험 · 실제 파일과 분리됨 · 변경 실행 불가". 띠 오른쪽에 "샘플 닫기"와 "실제 분석 시작". | `Notice`(info) + `Action` 2개. 전용 띠 부품은 없다(아래 참고). |
| 체험 결과 탐색 | 실제 탐색 화면과 같은 부품. 결과 제목 옆에 "예시" 표지. 파일 변경 동작은 비활성이고 사유 "샘플 데이터에서는 실행할 수 없습니다"를 보인다. | `StatusBadge`(info), `BrowseGrid`, `ItemActions` |
| 종료·초기화 확인 | "샘플 체험을 끝낼까요?" 본문에 체험 중 만든 선택과 보기 상태가 사라진다고 밝힌다. 취소가 먼저 초점을 받는다. | `ConfirmDialog` |
| 실제 분석으로 전환 | 샘플을 닫고 "새 분석" 화면으로 간다. 범위는 비워 두고 새로 검토하게 한다. 샘플 범위를 실제 범위로 넘기지 않는다. | `Action`(primary) |

초점 순서는 띠(문구 → 실제 분석 시작 → 샘플 닫기) 다음에 본문이다. 체험을 닫으면 진입 단추로 초점이 돌아간다.

**새로 필요한 부품**: 화면 위에 고정되는 전역 띠(배너). 지금은 `Notice`로 대신하되, 동작 단추를 옆에 둘 수 있는 띠가 공통 컴포넌트에 없다. 공통 컴포넌트 보드에도 띠와 예시 표지는 그려져 있지 않다. 보드에 있는 것은 단추 상태 6종, 상태 줄 5종(완료, 부분 완료, 실패, 오래된 관측, 확인 불가), 입력과 조건 칩, 색과 크기 규칙이다.

## 공통 규칙 (UX 계약 11, 공통 컴포넌트 보드)

- 상태는 색 + 기호 + 문구로 구분한다. 색만으로 구분하지 않는다.
- 빈 결과, 확인 불가, 오류는 서로 다른 문구로 쓴다. 모르는 값을 0으로 쓰지 않는다.
- 대화상자는 초점이 안에서 순환하고 Esc로 닫히며, 닫으면 연 단추로 초점이 돌아간다. 사라진 행이면 가까운 유효 행으로 간다.
- 초점 테두리는 2px로 보이고 선택 표시와 다른 모양이다.
- 기본 동작은 한 영역에 하나만 강조한다. 비활성 단추는 사유를 함께 보인다. 불러오는 동안 크기를 유지한다.
- 진행 막대는 실제 값만 반영한다. 총량을 모르면 불확정 막대와 개수만 보인다.
- 중요한 오류는 사라지지 않는 띠로 보인다.
- 644×461 창과 300% 확대에서는 한 열로 흐르고 가로 스크롤이 생기지 않아야 한다. 긴 이름과 경로는 줄바꿈하거나 가운데를 줄인다.

## 공통 컴포넌트 목록

`Action`, `Notice`, `StatusBadge`, `Progress`, `LiveStatus`, `ConfirmDialog`, `BrowseGrid`, `ItemActions`, `Heading`, `Text`, `Readout` (모두 `gui-web/src/components/index.ts`).
