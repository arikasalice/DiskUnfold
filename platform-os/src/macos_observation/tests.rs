//! Record parsing, classification and thread policy checks without fixture files.
use super::{bulk, dataless, errors, volume};
use du_core_contracts::{ports::PortErrorKind, scan_observation::ObservedKind};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const ERROR: u32 = 0x2000_0000;
const BASIC: u32 = libc::ATTR_CMN_RETURNED_ATTRS | libc::ATTR_CMN_NAME;

/// Packs one record: the error word precedes the name reference, fixed values
/// follow it, and the NUL-terminated name comes last.
fn record(
    returned: [u32; 5],
    error: Option<u32>,
    values: &[u8],
    name: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut out = vec![0u8; 4];
    for group in returned {
        out.extend(group.to_ne_bytes());
    }
    if let Some(code) = error {
        out.extend(code.to_ne_bytes());
    }
    let reference = out.len();
    out.extend([0u8; 8]);
    out.extend(values);
    let data = out.len();
    out.extend(name);
    out.push(0);
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
    let total = u32::try_from(out.len())?.to_ne_bytes();
    let offset = i32::try_from(data - reference)?.to_ne_bytes();
    let length = u32::try_from(name.len() + 1)?.to_ne_bytes();
    out.get_mut(0..4).ok_or("short")?.copy_from_slice(&total);
    out.get_mut(reference..reference + 4)
        .ok_or("short")?
        .copy_from_slice(&offset);
    out.get_mut(reference + 4..reference + 8)
        .ok_or("short")?
        .copy_from_slice(&length);
    Ok(out)
}

fn file_values() -> Vec<u8> {
    let mut values = Vec::new();
    values.extend(7u32.to_ne_bytes()); // device
    values.extend(1u32.to_ne_bytes()); // VREG
    values.extend(1_600_000_000i64.to_ne_bytes());
    values.extend(5_000_000i64.to_ne_bytes());
    values.extend(0u32.to_ne_bytes()); // flags
    values.extend(42u64.to_ne_bytes()); // file ID
    values.extend(2u32.to_ne_bytes()); // link count
    values.extend(3i64.to_ne_bytes()); // data length
    values.extend(4096i64.to_ne_bytes()); // data allocation
    values
}

#[test]
fn parses_every_requested_value_in_attribute_order() -> TestResult {
    let common = bulk::COMMON & !ERROR;
    let bytes = record(
        [common, 0, 0, bulk::FILE, 0],
        None,
        &file_values(),
        b"c.txt",
    )?;
    let records = bulk::parse(&bytes, 1).map_err(|e| format!("{e:?}"))?;
    let parsed = records.first().ok_or("missing record")?;
    assert_eq!(parsed.name, b"c.txt");
    assert_eq!(parsed.error, None);
    assert_eq!(parsed.device, Some(7));
    assert_eq!(parsed.kind(), ObservedKind::File);
    assert_eq!(parsed.modified, Some((1_600_000_000, 5_000_000)));
    assert_eq!(parsed.file_id, Some(42));
    assert_eq!(parsed.mount_status, None);
    assert_eq!(parsed.link_count, Some(2));
    assert_eq!(parsed.data_length, Some(3));
    assert_eq!(parsed.data_allocated, Some(4096));
    Ok(())
}

#[test]
fn values_that_were_not_returned_stay_unknown() -> TestResult {
    let bytes = record([BASIC, 0, 0, 0, 0], None, &[], b"only-name")?;
    let records = bulk::parse(&bytes, 1).map_err(|e| format!("{e:?}"))?;
    let parsed = records.first().ok_or("missing record")?;
    assert_eq!(
        parsed,
        &bulk::Record {
            name: b"only-name".to_vec(),
            ..Default::default()
        }
    );
    assert_eq!(parsed.kind(), ObservedKind::Unknown);
    Ok(())
}

#[test]
fn entry_error_precedes_the_name_and_consecutive_records_follow() -> TestResult {
    let mut bytes = record([BASIC | ERROR, 0, 0, 0, 0], Some(13), &[], b"denied")?;
    bytes.extend(record([BASIC, 0, 0, 0, 0], None, &[], b"next")?);
    let records = bulk::parse(&bytes, 2).map_err(|e| format!("{e:?}"))?;
    let names: Vec<_> = records.iter().map(|r| (r.name.clone(), r.error)).collect();
    assert_eq!(
        names,
        [(b"denied".to_vec(), Some(13)), (b"next".to_vec(), None)]
    );
    Ok(())
}

#[test]
fn malformed_or_unrequested_layouts_are_rejected() -> TestResult {
    let good = record([BASIC, 0, 0, 0, 0], None, &[], b"name")?;
    assert!(bulk::parse(&good, 2).is_err(), "count beyond the buffer");
    assert!(bulk::parse(good.get(..20).ok_or("short")?, 1).is_err());
    let fsid = record([BASIC | libc::ATTR_CMN_FSID, 0, 0, 0, 0], None, &[], b"x")?;
    assert!(bulk::parse(&fsid, 1).is_err(), "unrequested attribute");
    let volume = record([BASIC, 1, 0, 0, 0], None, &[], b"x")?;
    assert!(bulk::parse(&volume, 1).is_err(), "volume group returned");
    let unnamed = record([libc::ATTR_CMN_RETURNED_ATTRS, 0, 0, 0, 0], None, &[], b"x")?;
    assert!(bulk::parse(&unnamed, 1).is_err(), "name is required");
    let slash = record([BASIC, 0, 0, 0, 0], None, &[], b"a/b")?;
    assert!(bulk::parse(&slash, 1).is_err());
    let mut unterminated = good.clone();
    let last = unterminated
        .iter()
        .rposition(|&byte| byte == b'e')
        .ok_or("no name")?;
    *unterminated.get_mut(last + 1).ok_or("short")? = b'!';
    assert!(bulk::parse(&unterminated, 1).is_err());
    let mut tiny = good;
    tiny.get_mut(0..4)
        .ok_or("short")?
        .copy_from_slice(&8u32.to_ne_bytes());
    assert!(bulk::parse(&tiny, 1).is_err(), "length below the header");
    Ok(())
}

#[test]
fn directories_that_lead_elsewhere_are_links() {
    let directory = |mount_status, flags| bulk::Record {
        object_type: Some(2),
        mount_status,
        flags,
        ..Default::default()
    };
    assert_eq!(directory(Some(0), Some(0)).kind(), ObservedKind::Directory);
    assert_eq!(directory(Some(1), None).kind(), ObservedKind::Link);
    assert_eq!(directory(Some(2), None).kind(), ObservedKind::Link);
    assert_eq!(
        directory(Some(0), Some(0x0080_0000)).kind(),
        ObservedKind::Link
    );
    let typed = |object_type| bulk::Record {
        object_type: Some(object_type),
        ..Default::default()
    };
    assert_eq!(typed(5).kind(), ObservedKind::Link);
    assert_eq!(typed(7).kind(), ObservedKind::Other);
    assert_eq!(typed(0).kind(), ObservedKind::Unknown);
    assert_eq!(typed(8).kind(), ObservedKind::Unknown);
}

#[test]
fn errno_values_map_to_neutral_kinds_without_text() {
    let cases = [
        (
            libc::EPERM,
            PortErrorKind::PermissionDenied,
            "os.query_failed",
        ),
        (
            libc::EACCES,
            PortErrorKind::PermissionDenied,
            "os.query_failed",
        ),
        (libc::ENOENT, PortErrorKind::NotFound, "os.query_failed"),
        (libc::ENXIO, PortErrorKind::Io, "scan.disconnected"),
        (libc::EIO, PortErrorKind::Io, "os.query_failed"),
        (libc::ETIMEDOUT, PortErrorKind::Io, "os.query_failed"),
        (
            libc::ENOTDIR,
            PortErrorKind::Unsupported,
            "path.unresolvable",
        ),
        (libc::EDEADLK, PortErrorKind::Unsupported, "os.unsupported"),
    ];
    for (code, kind, reason) in cases {
        let error = errors::errno(code);
        assert_eq!((error.kind, error.reason_key.as_str()), (kind, reason));
        assert_eq!(error.os_code, Some(code));
    }
}

#[test]
fn timestamps_floor_to_milliseconds() {
    let text = |seconds, nanoseconds| {
        bulk::timestamp(seconds, nanoseconds).map(|time| time.as_str().to_owned())
    };
    assert_eq!(
        text(-1, 999_999_999).as_deref(),
        Some("1969-12-31T23:59:59.999Z")
    );
    assert_eq!(
        text(0, 999_999).as_deref(),
        Some("1970-01-01T00:00:00.000Z")
    );
    assert_eq!(text(i64::MAX, 0), None);
}

#[test]
fn materialization_is_refused_only_inside_the_call() -> TestResult {
    let (before, off) = dataless::thread_policy();
    let inside = dataless::without_materialization(|| dataless::thread_policy().0)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(inside, off);
    assert_eq!(dataless::thread_policy().0, before);
    Ok(())
}

#[test]
fn volume_reads_require_the_expected_identity_before_and_after() {
    let reads = std::cell::Cell::new(0);
    let wrong = volume::checked(
        b"expected",
        || Ok(b"other".to_vec()),
        || {
            reads.set(reads.get() + 1);
            Ok(1)
        },
    );
    assert_eq!(
        wrong.err().map(|e| e.kind),
        Some(PortErrorKind::IdentityChanged)
    );
    assert_eq!(reads.get(), 0);
    let calls = std::cell::Cell::new(0);
    let rebound = volume::checked(
        b"expected",
        || {
            calls.set(calls.get() + 1);
            Ok(if calls.get() == 1 {
                b"expected".to_vec()
            } else {
                b"other".to_vec()
            })
        },
        || Ok(1),
    );
    assert_eq!(
        rebound.err().map(|e| e.kind),
        Some(PortErrorKind::IdentityChanged)
    );
    let failure = errors::invalid();
    assert_eq!(
        volume::checked::<()>(b"same", || Ok(b"same".to_vec()), || Err(failure.clone())),
        Err(failure)
    );
}

/// Packs a reply to one volume attribute request after its returned groups.
fn volume_reply(
    returned: [u32; 5],
    values: &[&[u8]],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut out = vec![0u8; 4];
    for group in returned {
        out.extend(group.to_ne_bytes());
    }
    for value in values {
        out.extend(*value);
    }
    let total = u32::try_from(out.len())?.to_ne_bytes();
    out.get_mut(0..4).ok_or("short")?.copy_from_slice(&total);
    Ok(out)
}

#[test]
fn volume_replies_carry_one_attribute_after_the_file_system_id() -> TestResult {
    let fsid = [0x0e, 0, 0, 1, 0x1a, 0, 0, 0];
    let uuid = [
        0x20, 0x8c, 0x96, 0x9d, 0x12, 0x19, 0x43, 0x1c, 0x9d, 0x79, 0xab, 0x8f, 0x16, 0x61, 0xcd,
        0x8f,
    ];
    let common = libc::ATTR_CMN_RETURNED_ATTRS | libc::ATTR_CMN_FSID;
    let reply = volume_reply([common, libc::ATTR_VOL_UUID, 0, 0, 0], &[&fsid, &uuid])?;
    let parsed =
        volume::parse_volume::<16>(&reply, libc::ATTR_VOL_UUID).map_err(|e| format!("{e:?}"))?;
    assert_eq!((parsed.fsid, parsed.value), (fsid, Some(uuid)));
    assert_eq!(
        volume::format(parsed.fsid, parsed.value),
        b"uuid:208C969D-1219-431C-9D79-AB8F1661CD8F"
    );
    // Without a UUID the identity is the device number of the file system ID.
    let absent = volume_reply([common, 0, 0, 0, 0], &[&fsid])?;
    let parsed =
        volume::parse_volume::<16>(&absent, libc::ATTR_VOL_UUID).map_err(|e| format!("{e:?}"))?;
    assert_eq!(parsed.value, None);
    assert_eq!(volume::format(parsed.fsid, parsed.value), b"dev:0100000e");
    assert_eq!(volume::format(parsed.fsid, Some([0; 16])), b"dev:0100000e");
    // The kernel places space used before the UUID, so a reply carrying both
    // has another layout and is refused rather than misread.
    let both = volume_reply(
        [
            common,
            libc::ATTR_VOL_UUID | libc::ATTR_VOL_SPACEUSED,
            0,
            0,
            0,
        ],
        &[&fsid, &[0u8; 8], &uuid],
    )?;
    let no_fsid = volume_reply(
        [libc::ATTR_CMN_RETURNED_ATTRS, libc::ATTR_VOL_UUID, 0, 0, 0],
        &[&uuid],
    )?;
    let truncated = reply.get(..reply.len() - 1).ok_or("short")?;
    for malformed in [&both[..], &no_fsid, truncated] {
        assert!(volume::parse_volume::<16>(malformed, libc::ATTR_VOL_UUID).is_err());
    }
    Ok(())
}
