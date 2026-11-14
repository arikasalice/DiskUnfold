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
    assert_eq!(
        stderr,
        concat!(
            "저장된 설정을 사용할 수 없어 대체 언어를 사용합니다.\n",
            "DiskUnfold: 지원하지 않는 인자입니다. --help를 확인하세요.\n"
        )
        .as_bytes()
    );
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
