use super::*;

#[test]
fn valid_scan_keeps_the_normal_settings_read_warning() {
    let mut stored_reads = 0;
    let mut os_reads = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_io(
        ["scan", "root", "--", "--delete"]
            .into_iter()
            .map(OsString::from),
        || {
            stored_reads += 1;
            Err(())
        },
        || {
            os_reads += 1;
            Some(vec!["ko-KR".to_owned()])
        },
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit, ExitCode::from(2));
    assert_eq!(stored_reads, 1);
    assert_eq!(os_reads, 1);
    assert!(stdout.is_empty());
    let text = String::from_utf8_lossy(&stderr);
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("저장된 설정을 사용할 수 없어 대체 언어를 사용합니다.")
    );
    // The two paths after the option end do not exist: a usage error, in Korean.
    let error = lines.next().unwrap_or_default();
    assert!(
        error.starts_with("DiskUnfold: ")
            && error.contains("분석할 수 없는 경로가 있습니다.")
            && error.ends_with(" [INVALID_ARGUMENT]"),
        "{text}"
    );
    assert_eq!(lines.next(), None, "{text}");
}

#[test]
fn empty_arguments_skip_stored_settings_as_a_validation_error() {
    let mut stored_reads = 0;
    let mut os_reads = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_io(
        std::iter::empty(),
        || {
            stored_reads += 1;
            Err(())
        },
        || {
            os_reads += 1;
            Some(vec!["en-US".to_owned()])
        },
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit, ExitCode::from(2));
    assert_eq!(stored_reads, 0);
    assert_eq!(os_reads, 1);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"DiskUnfold: unsupported arguments. See --help.\n");
}

#[test]
fn invalid_explicit_language_value_is_not_reinterpreted_as_a_denied_option() {
    let mut stored_reads = 0;
    let mut os_reads = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_io(
        ["--lang", "--execute", "scan", "root"]
            .into_iter()
            .map(OsString::from),
        || {
            stored_reads += 1;
            Err(())
        },
        || {
            os_reads += 1;
            Some(vec!["ko-KR".to_owned()])
        },
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit, ExitCode::from(2));
    assert_eq!(stored_reads, 0);
    assert_eq!(os_reads, 0);
    assert!(stdout.is_empty());
    assert_eq!(stderr, LOCALE_UNSUPPORTED_LINE);
}

#[test]
fn duplicate_explicit_language_fails_before_settings_or_os_lookup() {
    let mut stored_reads = 0;
    let mut os_reads = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_io(
        ["--lang", "en", "--lang", "ko", "--delete"]
            .into_iter()
            .map(OsString::from),
        || {
            stored_reads += 1;
            Err(())
        },
        || {
            os_reads += 1;
            Some(vec!["ko-KR".to_owned()])
        },
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit, ExitCode::from(2));
    assert_eq!(stored_reads, 0);
    assert_eq!(os_reads, 0);
    assert!(stdout.is_empty());
    assert_eq!(stderr, LOCALE_UNSUPPORTED_LINE);
}
