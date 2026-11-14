# F13 미반영 로컬 커밋 이관 점검 (2026-11-14)

기준: `origin/0.0.1` (3fbdcbf7). 점검 대상 11개 커밋은 모두 `origin/0.0.1`의 조상이 아니다(`git merge-base --is-ancestor` 종료 코드 1).
방법: 커밋별·파일별로 커밋의 변경을 `git apply --check --reverse`로 현재 트리에 대어 이미 들어 있는지 확인하고, 맞지 않는 파일은 `git diff -w`와 키·문구 비교로 의미 차이를 읽었다. 문구는 후보 JSON의 키를 `user-env/i18n/message-keys.txt`, `en.json`, `ko.json`과 대조했다.

결론: 11개 커밋 모두 0.0.1에 없는 동작이 없다. 이관한 코드는 없고 `gui-web`도 바꾸지 않았다. 후보 파일(`f13-browse-candidate.json`, `f13-history-candidate.json`)은 가져오지 않았다.

| 커밋 | 내용 | 0.0.1 상태 | 조치 |
| --- | --- | --- | --- |
| d478e9c1 (codex/f13-browse-copy) | 탐색·시각화 화면 문구를 문구 키로 옮김 (BrowsePanel·BrowseResults·use-browse-copy, visualization 6개 파일, 후보 JSON) | 이미 있음. 14개 파일 중 4개(browse-display, use-browse-paging, use-item-inspection, GroupView)는 변경이 그대로 들어 있고, 나머지 9개는 `git diff -w` 기준으로 줄바꿈(Prettier)만 다르다. 후보 JSON 196개 키는 모두 `message-keys.txt`에 선언돼 있고 en·ko 문구가 같다. 예외는 영어 5개 키 문구(`ui.browse.copied_one_path_for`, `path_unrepresentable_for`, `path_multiline_for`, `clipboard_unsupported_for`, `clipboard_failed_for`)로, 0.0.1은 기존 문구를 유지하며 `gui-web/tests/browse-panel.test.tsx`(1501·1653·1687행)가 "Copied 1 path for Same." 등을 검증한다. | 이관 없음. 영어 문구 변경 여부는 제품 결정 사항으로 남김 |
| 9ec841e5 (codex/f13-history-copy) | 이력·설정 복원 화면 문구 키화 (HistoryPanel·HistoryDetailPanel·HistoryItems·history-labels·SettingsRestoration, 후보 JSON) | 이미 있음. 3개 파일은 변경이 그대로 들어 있다. 후보 JSON 182행 중 181행이 0.0.1 en·ko 문구와 같다. 남은 1행(`ui.history.one_record_shown`)은 0.0.1이 선언된 복수형 키 `ui.history.records_shown` 하나(en one/other, ko other)로 대체했다. SettingsRestoration의 진행 표시 키는 0.0.1이 `ui.settings.loading_restoration_progress`로 고쳤다. | 이관 없음 |
| 4a0b8b36 (codex/f13-linux-desktop) | Linux 데스크톱 지역 설정 조립: app-host 수명 주기·시작 DB·공유 DB·linux_instance_lock, gui-web 브리지(`hasDesktopLocaleBridge`, `nativeLocaleHost`) | 이미 있음, 더 강함. `linux_instance_lock.rs`와 `settings.rs` 변경은 그대로 들어 있다. app-host는 `linux`·`macos`까지 cfg를 넓혀 `linux_ui_language`·`LinuxTimeZone`을 조립하고, `startup_database/linux_tests.rs`는 커밋의 4개 시험을 모두 포함하며 1개가 더 있다. gui-web은 `hasDesktopLocaleBridge`가 `diskunfold://app/index.html`에 `__diskunfoldLocale` 표지까지 요구하고 `ApplicationEntry`·`DesktopLocale`이 `analysisEnabled`와 `nativeLocaleHost`를 쓴다. | 이관 없음 |
| cc98633f (codex/f13-linux-desktop) | Linux 로컬 시간대 관측 (`TimeZoneId::Linux`, `linux_time_zone.rs`) | 이미 있음. 세 파일 중 `time_zone.rs`·`linux_time_zone.rs` 변경이 그대로 들어 있고 `platform-os/src/lib.rs`는 모듈 줄만 다르다. | 이관 없음 |
| 1298f241 (codex/f13-bundle-684638e) | 묶음 등록부 후보: 서식 표·복수 규칙·문장 틀 해석기(`MessageTemplate`·`format_message`)·언어 간 매개변수 종류 일치 검사 | 다른 방식으로 이미 있음. 0.0.1은 core-contracts의 `MessageTemplate`, user-env의 `render_text`·`resolve_message`·`resolve_reason`, 서식 표 검증, 복수 규칙, 대체 사슬, `value.unknown` 처리를 갖췄다. 중괄호 이스케이프는 `template.rs` 시험, 복수·알 수 없는 값은 `embedded_history_uses_typed_plural_rules_and_unknown_values`, 대체 표지는 이번 PR의 `cli/src/fallback_tests.rs`가 확인한다. 실행 시 언어 간 매개변수 불일치 거절은 0.0.1에 없지만, 설계(NFR2.HD1)는 병합 단계 대조로 정하고 실행 시에는 `value.unknown`으로 채우라고 하므로(FR6.HD1.app-host.DD1) 가져오지 않았다. 병합 단계 대조는 `parameter_mismatches`와 `built_in_resources_register_with_exact_declared_keys_and_parameters`가 한다. | 이관 없음 |
| f21e736a (codex/f13-bundle-684638e) | 위 후보의 문장 틀 반복 해석 제거 (`register_embedded`) | 해당 없음. 위 후보의 API(`parse_bundle_with_templates`)가 0.0.1에 없고 0.0.1 등록은 묶음당 한 번만 해석한다. | 이관 없음 |
| 727a12e0 | 내장 언어 묶음 자원: build.rs·resources.txt·embedded.rs | 이미 있음, 더 넓음. `resources.txt` 변경은 그대로 들어 있다. 0.0.1은 build.rs로 묶음 해시와 사유 메타 표를 내장하고, 해시 변조·형식 표·대체 언어 시험까지 갖췄다. | 이관 없음 |
| 216c9285 | 순수 시작 언어 결정 (`user-env/src/locale`) | 이미 있음, 더 넓음. `decide`·`decide_cli`·`primary_language`·`descriptors`가 있고 0.0.1이 `DISPLAY_LANGUAGE_KEY`, 인자 정규화(`KO_kr`), 형식기(`format`)를 더했다. | 이관 없음 |
| d2a3efc6 | 시작 언어 결정 시험 3개 | 같은 경계를 이미 시험함. 첫 OS 태그만 쓰는 규칙(`os_selection_uses_only_the_first_tag_and_capability`), 저장·인자 우선순위와 잘못된 값(`stored_and_cli_precedence_and_invalid_values_are_distinct`), 한국어 무결성 실패(`embedded_korean_integrity_failure_preserves_the_configured_language`, `cli/src/startup_locale.rs`의 두 시험)가 같은 동작을 확인한다. 커밋의 시험은 0.0.1에 없는 `embedded::DECLARED_KEYS` 상수를 쓴다. | 이관 없음 |
| f6708a3e | 시작 언어 적재 경계 (`gui-web/src/state/locale-startup.ts`와 시험) | 대체됨. 어디서도 쓰이지 않던 일반화된 경계였고, 0.0.1의 `use-locale.ts`가 같은 일을 한다: 프로필 구독 후 읽기, 영어 기준 묶음 먼저 적재, 선택 언어 실패 시 영어 유지와 오류 표시, 묶음 판 캐시와 `knownBundleRevision`, 이벤트 판 비교 후 다시 읽기. | 이관 없음 |
| 74d107df | Windows 선호 UI 언어 읽기 (`windows_ui_language`) | 이미 있음. `windows_ui_language.rs`와 시험이 그대로 들어 있고 `platform-os/src/lib.rs`는 모듈 줄만 다르다. | 이관 없음 |

## 이 점검에서 보인 다른 사항 (커밋 대상 아님)

- `gui-web/src/components/`는 건드리지 않았고 이관 항목에도 공통 부품 변경이 없다.
- FR6.HD2.app-host.DD1은 세션 누락 목록이 1,000건을 넘길 때 "버린 수를 센다"고 정한다. `user-env/src/i18n/missing.rs`는 가장 오래된 항목을 버리기만 하고 버린 수는 세지 않는다. user-env 구현 경로라 이번 PR에서 바꾸지 않았다.
