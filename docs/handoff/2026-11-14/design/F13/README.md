# F13 다국어와 지역화 · 디자인 자료 (2026-11-14)

Penpot 파일 `e280b017-e522-80cb-8008-e0ee2a1f5e75`의 보드를 내보내기 API로 1배율 PNG로 받았다. 그림은 편집하지 않았다.

**한계부터 밝힌다.** Penpot에는 이 기능의 화면 보드가 테마별로 한 장씩만 있다. 상태별 보드(불러오는 중, 빈 결과, 오류 등)는 그려져 있지 않다. 아래 "상태와 배치" 절은 발행 HLD의 화면 계약과 Penpot의 UX 계약 문구, 기존 공통 컴포넌트를 근거로 디자인팀이 글로 정한 것이며, 그림으로 검증된 내용이 아니다. 보드의 수치와 이름은 예시 데이터다. 왼쪽 탐색 메뉴가 있는 1440×960 셸은 현재 제품 셸과 다르므로 본문 영역의 구성만 기준으로 삼는다.

## 파일

| 내용 | PNG | Penpot |
| --- | --- | --- |
| settings 화면 (모양과 언어) | [Light](settings-light.png) · [Dark](settings-dark.png) | [Light](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961e9d5a&board-id=b330ca3c-df1a-80f0-8008-e10d83977901) · [Dark](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961f2e17&board-id=b330ca3c-df1a-80f0-8008-e10dc996be9a) |
| UX 계약 09 · 설정·테마·지역화 (F13 포함) | [PNG](ux-contract-09-settings-locale-F13.png) | [Penpot](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961fff7f&board-id=b330ca3c-df1a-80f0-8008-e10e84891ca3) |
| 공통 컴포넌트 | [Light](components-common-light.png) · [Dark](components-common-dark.png) | [Penpot Light](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961f5601&board-id=b330ca3c-df1a-80f0-8008-e10f064233bc) · [Dark](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961f5601&board-id=b330ca3c-df1a-80f0-8008-e10f07beb07b) |
| 확인 대화상자 예시 | [Light](dialog-confirm-light.png) · [Dark](dialog-confirm-dark.png) | [Penpot Light](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961e9d5a&board-id=b330ca3c-df1a-80f0-8008-e10f4b84b94e) · [Dark](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961f2e17&board-id=b330ca3c-df1a-80f0-8008-e10f683db782) |
| UX 계약 11 · 공통 상태·접근성·모션 | [PNG](ux-contract-11-common-states.png) | [Penpot](http://192.168.1.210:9001/#/workspace?team-id=e280b017-e522-80cb-8008-e0eb195b43ab&file-id=e280b017-e522-80cb-8008-e0ee2a1f5e75&page-id=b330ca3c-df1a-80f0-8008-e10c961fff7f&board-id=b330ca3c-df1a-80f0-8008-e10e86b29e39) |

## 보드에 그려진 것

settings 보드는 왼쪽 설정 범주 목록과 오른쪽 "모양과 언어" 패널이다.

- 테마: 시스템 설정 따르기 / Light / Dark 세 선택지.
- 설정 표: 열은 설정 · 현재 값 · 적용. 행은 표시 언어(한국어, 즉시), 용량 단위(SI · 자동, 즉시), 모션(시스템 설정 따르기, 즉시), 기본 보기(트리맵 + 목록, 다음 보기).
- 아래에 "변경 검토·적용"(기본 동작)과 "초기화 검토".

UX 계약 09에서 F13에 해당하는 문장: "문자열 키·복수형·숫자·날짜·단위 형식 분리. 경로는 번역하지 않음. 한국어 IME 조합 보호, 영어 확장 공간 확보. 기계용 원시 값과 사용자 표시값 분리." 모션은 "System(기본)/줄이기 제공. OS reduce-motion이면 위치 이동·확대·반복 효과 제거. 상태 변화는 즉시 텍스트로 표시."

요청하신 네 가지 중 보드에 있는 것은 표시 언어와 모션 행뿐이다. 날짜·숫자 형식, 화면 배율, 번역 미확인 표지, 긴 문구와 가운데 줄임이 적용된 부품은 그림이 없다.

## 상태와 배치 (글로 정함)

| 항목 | 배치와 규칙 | 공통 컴포넌트 |
| --- | --- | --- |
| 언어·지역 형식 | "모양과 언어" 패널의 설정 표에 행을 더한다: 표시 언어, 날짜 형식, 숫자 형식, 시간대. 각 행은 현재 값 선택과 적용 시점을 보인다. 값 아래에 예시 한 줄(예: 2026.11.14 14:32, 1,234.5)을 둔다. | `SelectField`, `Readout`, `Text`(muted) |
| 화면 배율 | 같은 표에 "화면 배율" 행. 선택하면 즉시 적용하고, 되돌릴 수 있게 이전 값을 보인다. 배율이 커져도 가로 스크롤이 생기지 않아야 한다. | `SelectField` |
| 움직임 줄이기 | "모션" 행. 선택지는 시스템 설정 따르기 / 줄이기. OS가 줄이기면 선택과 관계없이 줄인다는 설명을 둔다. | `SelectField`, `Text`(muted) |
| 번역 미확인 표지 | 번역이 확인되지 않은 문구가 있는 언어를 고르면, 언어 행 아래에 정보 띠 "일부 문구는 번역을 확인하지 못해 영어로 표시됩니다". 개별 문구에는 표지를 붙이지 않는다. | `Notice`(info). 언어 선택지 옆 `StatusBadge`(neutral) "일부 미확인" |
| 긴 문구 | 단추는 줄바꿈을 허용하고 글자를 자르지 않는다. 표 셀은 줄바꿈하고 행 높이가 늘어난다. 경로는 가운데를 줄이고 전체 값을 상세나 도구 설명으로 제공한다. 한국어 본문은 낱말 중간에서 끊지 않는다. | `Action`, `DataTable`, `PathField`, `Text`(path) |

언어를 바꿔도 현재 화면과 초점 위치는 유지한다. 입력 중인 한글 조합을 끊지 않는다.

**새로 필요한 부품**: 가운데 줄임을 하는 경로 표시. 지금 `Text kind="path"`는 끝에서 줄바꿈만 한다. 공통 컴포넌트로 하나 두는 편이 맞다.

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

`SelectField`, `ChoiceGroup`/`Choice`(테마 세 선택지), `Action`, `Notice`, `StatusBadge`, `Readout`, `DataTable`, `PathField`, `Text`, `Heading`, `ReviewSection`, `ConfirmDialog`(초기화 검토).
