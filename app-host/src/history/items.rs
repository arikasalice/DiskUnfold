//! Project persisted item pages through the read-only history connection.

use super::{HistoryReader, checked_counter, db, published_counter};
use du_core_contracts::{Id, history_ipc::HistoryItemOutcome};
use du_engine_api::history::{
    Revision,
    items::{Cursor, Page, Read, ReadError},
};

impl Read for HistoryReader {
    fn page(
        &self,
        job: &Id,
        outcome: Option<HistoryItemOutcome>,
        cursor: Option<Cursor>,
        count: usize,
    ) -> Result<Page, ReadError> {
        let page = self
            .connection
            .lock()
            .map_err(|_| ReadError::StorageUnavailable)?
            .read_history_items(
                job,
                outcome,
                cursor
                    .map(|cursor| stored_cursor(job, outcome, cursor))
                    .transpose()?,
                count,
            )
            .map_err(|error| match error {
                db::items::Error::NotFound => ReadError::Removed,
                db::items::Error::VersionChanged => ReadError::VersionChanged,
                _ => ReadError::StorageUnavailable,
            })?;
        Ok(Page {
            revision: published_revision(page.revision)?,
            rows: page
                .rows
                .into_iter()
                .map(|row| Ok((number(row.item_seq)?, row.item)))
                .collect::<Result<_, _>>()?,
            next: page
                .next
                .map(|cursor| {
                    Ok(Cursor {
                        revision: published_revision(cursor.revision)?,
                        last: number(cursor.last_item_seq)?,
                        upper: number(cursor.max_item_seq)?,
                    })
                })
                .transpose()?,
            summary: page.summary,
            recorded: page.recorded,
            item_history_expected: page.item_history_expected,
        })
    }
}

fn stored_cursor(
    job: &Id,
    filter: Option<HistoryItemOutcome>,
    cursor: Cursor,
) -> Result<db::items::Cursor, ReadError> {
    let signed = |value| checked_counter(value).map_err(|_| ReadError::StorageUnavailable);
    Ok(db::items::Cursor {
        job_id: job.clone(),
        revision: db::Revision {
            delete_generation: signed(cursor.revision.delete_generation)?,
            max_record_seq: signed(cursor.revision.max_record_seq)?,
        },
        last_item_seq: signed(cursor.last)?,
        max_item_seq: signed(cursor.upper)?,
        filter,
    })
}

fn number(value: i64) -> Result<u64, ReadError> {
    published_counter(value).map_err(|_| ReadError::StorageUnavailable)
}

fn published_revision(revision: db::Revision) -> Result<Revision, ReadError> {
    Ok(Revision {
        delete_generation: number(revision.delete_generation)?,
        max_record_seq: number(revision.max_record_seq)?,
    })
}
