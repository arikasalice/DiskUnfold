//! Single-file read and metadata verification.
use crate::{
    candidates::{self, SampleRange, Size},
    read_measurement::ReadMeasurement,
    verification_input::{Candidate, VerificationTarget, valid_approval},
    verification_ports::{Digest, HashPurpose, HashStrategy, OpenFile, ReadError, ReadPort},
    verification_result::State,
};
pub(crate) fn read_digest(
    target: &Candidate<'_>,
    purpose: HashPurpose,
    reader: &mut impl ReadPort,
    hasher: &dyn HashStrategy,
    measurement: &mut ReadMeasurement,
) -> Result<Vec<u8>, State> {
    let expected = target.approved.snapshot.as_ref().ok_or(State::Unchecked)?;
    let mut file = reader
        .open_bound(target.approved.target.item_no, &target.identity)
        .map_err(read_error)?;
    if file.snapshot().map_err(read_error)? != *expected {
        return Err(State::Changed);
    }
    let mut digest = hasher.start(purpose, expected.size);
    match purpose {
        HashPurpose::Sample => {
            let ranges = candidates::sample_ranges(expected.size).ok_or(State::ReadFailed)?;
            for range in ranges {
                read_range(&mut *file, range, &mut *digest, measurement)?;
            }
        }
        HashPurpose::Full => {
            read_range(
                &mut *file,
                SampleRange {
                    offset: 0,
                    length: expected.size,
                },
                &mut *digest,
                measurement,
            )?;
            let mut probe = [0_u8; 1];
            if measured_read(&mut *file, expected.size, &mut probe, measurement)? != 0 {
                return Err(State::Changed);
            }
        }
    }
    if file.snapshot().map_err(read_error)? != *expected {
        return Err(State::Changed);
    }
    let digest = digest.finish();
    if digest.is_empty() {
        return Err(State::ReadFailed);
    }
    Ok(digest)
}

/// Produces a full digest only through the normal identity-bound before/after checks.
pub(crate) fn verified_full_for_reuse(
    target: &VerificationTarget,
    reader: &mut impl ReadPort,
    hasher: &dyn HashStrategy,
) -> Result<Vec<u8>, State> {
    let identity = target.target.identity.as_ref().ok_or(State::Unchecked)?;
    let Size::Exact(size) = target.target.size else {
        return Err(State::Unchecked);
    };
    if target.target.kind != candidates::EntryKind::File
        || identity.volume.is_empty()
        || identity.file.is_empty()
        || !valid_approval(target, identity, size)
    {
        return Err(State::Unchecked);
    }
    let candidate = Candidate {
        approved: target,
        aliases: vec![target.target.item_no],
        identity: identity.clone(),
    };
    read_digest(
        &candidate,
        HashPurpose::Full,
        reader,
        hasher,
        &mut ReadMeasurement::default(),
    )
}
fn read_range(
    file: &mut dyn OpenFile,
    range: SampleRange,
    digest: &mut dyn Digest,
    measurement: &mut ReadMeasurement,
) -> Result<(), State> {
    let mut remaining = range.length;
    let mut offset = range.offset;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let limit =
            usize::try_from(remaining.min(buffer.len() as u64)).map_err(|_| State::ReadFailed)?;
        let count = measured_read(file, offset, &mut buffer[..limit], measurement)?;
        if count == 0 {
            return Err(State::ReadFailed);
        }
        digest.update(&buffer[..count]);
        let count = count as u64;
        offset = offset.checked_add(count).ok_or(State::ReadFailed)?;
        remaining -= count;
    }
    Ok(())
}
fn measured_read(
    file: &mut dyn OpenFile,
    offset: u64,
    buffer: &mut [u8],
    measurement: &mut ReadMeasurement,
) -> Result<usize, State> {
    match file.read_at(offset, buffer) {
        Ok(count) => {
            measurement.record(count, buffer.len());
            if count > buffer.len() {
                Err(State::ReadFailed)
            } else {
                Ok(count)
            }
        }
        Err(error) => {
            measurement.exact = false;
            Err(read_error(error))
        }
    }
}
fn read_error(error: ReadError) -> State {
    match error {
        ReadError::Failed => State::ReadFailed,
        ReadError::IdentityChanged => State::Changed,
    }
}
