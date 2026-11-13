use super::*;
use std::cell::{Cell, RefCell};

struct Fake {
    generation: Cell<u64>,
    last_count: Cell<usize>,
    revisions: RefCell<Vec<Option<Revision>>>,
}

impl HistoryRead for Fake {
    type Summary = u64;

    fn page(
        &self,
        _: &Filter,
        revision: Option<Revision>,
        after: Option<Position>,
        count: usize,
    ) -> Result<ReadPage<u64>, ReadError> {
        self.last_count.set(count);
        self.revisions.borrow_mut().push(revision);
        let pinned = revision.unwrap_or(Revision {
            delete_generation: self.generation.get(),
            max_record_seq: 3,
        });
        if pinned.delete_generation != self.generation.get() {
            return Err(ReadError::VersionChanged);
        }
        let entries: Vec<_> = (1..=3)
            .rev()
            .map(|n| Entry {
                position: Position {
                    reckon_at_ms: 10,
                    record_seq: n,
                },
                summary: if n == 2 { None } else { Some(n) },
            })
            .filter(|e| after.is_none_or(|p| e.position < p))
            .take(count)
            .collect();
        let has_more = entries.last().is_some_and(|e| e.position.record_seq > 1);
        Ok(ReadPage {
            revision: pinned,
            entries,
            has_more,
        })
    }
}

fn make_session(key: u8) -> HistoryList<Fake> {
    HistoryList::new(
        Fake {
            generation: Cell::new(0),
            last_count: Cell::new(0),
            revisions: RefCell::new(vec![]),
        },
        [key; 32],
    )
}

#[test]
fn authenticated_page_pins_filter_revision_and_preserves_missing_rows() -> Result<(), ListError> {
    let session = make_session(7);
    let filter = Filter {
        kinds: vec!["scan.metadata".into()],
        ..Filter::default()
    };
    let first = session.list(&filter, None, Some(1))?;
    let cursor = first.next_cursor.ok_or(ListError::StorageUnavailable)?;
    let second = session.list(&filter, Some(&cursor), Some(1))?;
    assert_eq!(second.entries[0].summary, None);
    assert_eq!(second.revision, first.revision);
    assert_eq!(
        session.list(&Filter::default(), Some(&cursor), None).err(),
        Some(ListError::InvalidCursor)
    );
    assert_eq!(
        make_session(8).list(&filter, Some(&cursor), None).err(),
        Some(ListError::InvalidCursor)
    );
    let mut tampered = cursor.clone();
    tampered.push('x');
    assert_eq!(
        session.list(&filter, Some(&tampered), None).err(),
        Some(ListError::InvalidCursor)
    );
    session.reader.generation.set(1);
    assert_eq!(
        session.list(&filter, Some(&cursor), None).err(),
        Some(ListError::VersionChanged)
    );
    Ok(())
}

#[test]
fn limits_and_invalid_reader_page_fail_closed() -> Result<(), ListError> {
    let session = make_session(7);
    let _ = session.list(&Filter::default(), None, None)?;
    assert_eq!(session.reader.last_count.get(), 200);
    let _ = session.list(&Filter::default(), None, Some(1000))?;
    assert_eq!(session.reader.last_count.get(), 1000);
    assert_eq!(
        session.list(&Filter::default(), None, Some(0)).err(),
        Some(ListError::InvalidLimit)
    );
    assert_eq!(
        session.list(&Filter::default(), None, Some(1001)).err(),
        Some(ListError::InvalidLimit)
    );
    let revision = Revision {
        delete_generation: 0,
        max_record_seq: 3,
    };
    let invalid = ReadPage {
        revision,
        entries: vec![Entry {
            position: Position {
                reckon_at_ms: 1,
                record_seq: 4,
            },
            summary: Some(()),
        }],
        has_more: false,
    };
    assert_eq!(
        validate_page(&invalid, None, 1),
        Err(ListError::StorageUnavailable)
    );
    Ok(())
}

#[test]
fn malformed_reader_order_and_revision_are_rejected() {
    let revision = Revision {
        delete_generation: 2,
        max_record_seq: 5,
    };
    let position = Position {
        reckon_at_ms: 42,
        record_seq: 3,
    };
    let repeated = ReadPage {
        revision,
        entries: vec![Entry {
            position,
            summary: Some(()),
        }],
        has_more: false,
    };
    assert_eq!(
        validate_page(&repeated, Some((revision, position)), 10),
        Err(ListError::StorageUnavailable)
    );
    let old = Revision {
        delete_generation: 1,
        max_record_seq: 5,
    };
    assert_eq!(
        validate_page(&repeated, Some((old, position)), 10),
        Err(ListError::VersionChanged)
    );
}
