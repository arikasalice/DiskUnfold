use super::*;

pub(super) fn check(api: &FolderBrowse, expected: &Id) -> Result<(), String> {
    let mut polls = 0;
    assert_eq!(
        api.open(request(None), || {
            polls += 1;
            polls == 2
        }),
        Err(Error::Cancelled)
    );
    assert_eq!(&open(api, request(None))?.page.query_ref, expected);
    Ok(())
}
