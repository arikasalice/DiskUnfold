use super::*;
use crate::records::{NameEncoding, gap::Gap};
use du_core_contracts::PathBytes;

fn wrapped(records: &[u8], metadata: &str) -> Result<VerifiedSpool<MemorySealed>, String> {
    let compressed = frame(records, true).map_err(|e| e.to_string())?;
    let mut data = b"DUSNAP\r\n".to_vec();
    data.extend(1u16.to_le_bytes());
    data.extend(0u16.to_le_bytes());
    data.extend((metadata.len() as u32).to_le_bytes());
    data.extend(metadata.as_bytes());
    data.extend(&compressed);
    data.extend((compressed.len() as u64).to_le_bytes());
    data.extend(3u64.to_le_bytes());
    data.extend((records.len() as u64).to_le_bytes());
    data.extend(blake3::hash(&data).as_bytes());
    sealed(&data)
}
fn metadata(encoding: &str, roots: &[&[u8]]) -> String {
    let roots: Vec<_> = roots
        .iter()
        .map(|x| PathBytes::from_bytes(x).as_str().to_owned())
        .collect();
    serde_json::json!({"pathEncoding":encoding,"source":{"scopeRoots":roots}}).to_string()
}
fn records(encoding: NameEncoding, path: &[u8]) -> Result<Vec<u8>, String> {
    let body = Gap {
        root_index: 1,
        path: vec![path],
        reason_key: "scan.denied",
        observed_at_ms: None,
    }
    .encode(encoding, 2)
    .map_err(|e| format!("{e:?}"))?;
    let mut result = vec![1, 0, 0, 0, 1];
    result.extend(((body.len() + 1) as u32).to_le_bytes());
    result.push(3);
    result.extend(body);
    result.extend([1, 0, 0, 0, 127]);
    Ok(result)
}
#[test]
fn derives_roots_from_same_sealed_copy_preserving_order_and_bytes() -> Result<(), String> {
    let input = records(NameEncoding::Bytes, b"\xff")?;
    let a = b"/\xff".as_slice();
    let b = b"/other".as_slice();
    let meta = metadata("bytes", &[a, b]);
    let mut spool = wrapped(&input, &meta)?;
    let (stage, roots) = validate_items_and_gaps_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stage.bytes, input);
    assert_eq!(roots.roots(), [a, b]);
    assert_eq!(roots.encoding(), NameEncoding::Bytes);
    let windows = |s: &str| {
        s.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    let w1 = windows("C:\\one");
    let w2 = windows("D:\\two");
    let path = [b'x', 0];
    let input = records(NameEncoding::WindowsUtf16Le, &path)?;
    let mut spool = wrapped(&input, &metadata("utf16le", &[&w1, &w2]))?;
    let (_, roots) = validate_items_and_gaps_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(roots.roots(), [w1, w2]);
    assert_eq!(roots.encoding(), NameEncoding::WindowsUtf16Le);
    Ok(())
}
#[test]
fn bad_metadata_or_mismatched_roots_discards_staging() -> Result<(), String> {
    let input = records(NameEncoding::Bytes, b"file")?;
    for (meta, is_metadata) in [
        (metadata("bytes", &[b"/one"]), false),
        (metadata("bytes", &[b"relative", b"/other"]), true),
        (metadata("utf16le", &[b"/one", b"/other"]), true),
        (
            "{\"pathEncoding\":\"bytes\",\"source\":{\"scopeRoots\":[\"Lw==\"]}}".to_owned(),
            true,
        ),
        ("{}".to_owned(), true),
    ] {
        let mut spool = wrapped(&input, &meta)?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        let result = validate_items_and_gaps_in_sealed(&mut spool, stage, || false);
        assert!(if is_metadata {
            matches!(result, Err(Error::Metadata))
        } else {
            matches!(result, Err(Error::Records))
        });
        assert_eq!(discarded.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn duplicate_metadata_keys_are_rejected_and_stage_discarded() -> Result<(), String> {
    let input = records(NameEncoding::Bytes, b"file")?;
    for meta in [
        r#"{"pathEncoding":"bytes","pathEncoding":"utf16le","source":{"scopeRoots":[]}}"#,
        r#"{"pathEncoding":"bytes","source":{"scopeRoots":[],"scopeRoots":[]}}"#,
        r#"{"pathEncoding":"bytes","source":{"scopeRoots":[]},"source":{"scopeRoots":[]}}"#,
        r#"{"pathEncoding":"bytes","path\u0045ncoding":"bytes","source":{"scopeRoots":[]}}"#,
        r#"{"pathEncoding":"bytes","source":{"scopeRoots":[],"other":{"x":1,"x":2}}}"#,
    ] {
        let mut spool = wrapped(&input, meta)?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        assert!(matches!(
            validate_items_and_gaps_in_sealed(&mut spool, stage, || false),
            Err(Error::Metadata)
        ));
        assert_eq!(discarded.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn windows_namespaces_keep_original_bytes_and_reject_device_paths() -> Result<(), String> {
    let wide = |s: &str| {
        s.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    let input = records(NameEncoding::WindowsUtf16Le, &[b'x', 0])?;
    let drive = wide(r"C:\folder");
    let extended = wide(r"\\?\D:\other");
    let mut unc = wide(r"\\?\UNC\server\share\");
    unc.extend(0xd800u16.to_le_bytes());
    let volume = wide(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\folder");
    let mut spool = wrapped(
        &input,
        &metadata("utf16le", &[&drive, &extended, &unc, &volume]),
    )?;
    let (_, roots) = validate_items_and_gaps_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(roots.roots(), [drive, extended, unc, volume]);
    for bad in [
        r"\\.\C:\",
        r"\\.\PhysicalDrive0",
        r"\\?\GLOBALROOT\Device\HarddiskVolume1\",
        r"\\?\\\host\share",
        r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}",
        r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdeg}\",
        r"\\host\..\folder",
        r"UNC\host\share",
        r"C:relative",
        r"C:\one\..\two",
        r"C:\one:stream",
    ] {
        let root = wide(bad);
        let mut spool = wrapped(&input, &metadata("utf16le", &[&root, &root]))?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        assert!(
            matches!(
                validate_items_and_gaps_in_sealed(&mut spool, stage, || false),
                Err(Error::Metadata)
            ),
            "{bad}"
        );
        assert_eq!(discarded.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    Ok(())
}
