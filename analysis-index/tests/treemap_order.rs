//! Display order of equal-size treemap siblings: raw name bytes, never a comparison key.
use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{squarify::Rect, treemap},
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};
use std::sync::Arc;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn logical(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}
fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn add_files(
    build: &mut du_analysis_index::BuildHandle,
    files: &[(&[u8], u64)],
) -> Result<(), String> {
    for (name, bytes) in files {
        build
            .add_file(Observation {
                path: vec![name.to_vec()],
                size: Some(logical(*bytes)),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}
fn sealed(files: &[(&[u8], u64)]) -> Result<Arc<ResultVersion>, String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    add_files(&mut build, files)?;
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))
}
/// Raw names of the children of item 0 in the order the treemap admits them.
fn laid_out(version: &ResultVersion) -> Result<Vec<Vec<u8>>, String> {
    let parent = ItemRef {
        result_version: version.version_id().clone(),
        item_no: 0,
    };
    let bounds = Rect {
        left: 0,
        top: 0,
        right: 1_000,
        bottom: 1_000,
    };
    let partition = treemap::layout_parent(version, &parent, SizeBasis::Logical, bounds)
        .map_err(|e| format!("{e:?}"))?;
    partition
        .children
        .iter()
        .map(|child| {
            version
                .items()
                .get(child.item.item_no as usize)
                .map(|item| item.name.clone())
                .ok_or_else(|| "unknown child".to_owned())
        })
        .collect()
}

#[test]
fn equal_size_siblings_follow_raw_name_bytes() -> Result<(), String> {
    // Inserted out of order; every name has the same size.
    let names: [&[u8]; 15] = [
        b"\xff\xff",
        b"b",
        b"abc",
        "あ".as_bytes(),
        b"\x80",
        b"a\x00\x00",
        b"a",
        b"\xff",
        b"z",
        "é".as_bytes(),
        b"ab",
        b"a\x00",
        b"B",
        b"\xc3",
        b"A",
    ];
    let files: Vec<(&[u8], u64)> = names.iter().map(|name| (*name, 10)).collect();
    let expected: [&[u8]; 15] = [
        b"A",
        b"B",
        b"a",
        b"a\x00",
        b"a\x00\x00",
        b"ab",
        b"abc",
        b"b",
        b"z",
        b"\x80",
        b"\xc3",
        b"\xc3\xa9",
        b"\xe3\x81\x82",
        b"\xff",
        b"\xff\xff",
    ];
    let version = sealed(&files)?;
    assert_eq!(laid_out(&version)?, expected);
    Ok(())
}

#[test]
fn utf16le_names_order_by_their_bytes_not_by_text() -> Result<(), String> {
    // U+0100 is bytes 00 01, so it sorts before "a" (61 00) although its text sorts after.
    let (a, b, wide) = (utf16le("a"), utf16le("b"), utf16le("\u{100}"));
    let files = [(b.as_slice(), 7), (a.as_slice(), 7), (wide.as_slice(), 7)];
    let expected = [wide.clone(), a.clone(), b.clone()];
    let version = sealed(&files)?;
    assert_eq!(laid_out(&version)?, expected);
    Ok(())
}

#[test]
fn size_decides_first_and_names_only_break_ties_including_zero_sizes() -> Result<(), String> {
    let files: [(&[u8], u64); 6] = [
        (b"m", 5),
        (b"k", 20),
        (b"z", 20),
        (b"c", 0),
        (b"b", 5),
        (b"a", 0),
    ];
    let expected: [&[u8]; 6] = [b"k", b"z", b"b", b"m", b"a", b"c"];
    let version = sealed(&files)?;
    assert_eq!(laid_out(&version)?, expected);
    Ok(())
}

#[test]
fn later_version_of_one_build_keeps_the_relative_order_of_existing_siblings() -> Result<(), String>
{
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    add_files(&mut build, &[(b"f", 10), (b"b", 10), (b"d", 10)])?;
    let partial = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    // New equal-size siblings arrive before, between and after the existing ones.
    add_files(
        &mut build,
        &[(b"e", 10), (b"a", 10), (b"c", 10), (b"g", 10)],
    )?;
    let next = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let before = laid_out(&partial)?;
    assert_eq!(before, [b"b".to_vec(), b"d".to_vec(), b"f".to_vec()]);
    let after = laid_out(&next)?;
    let existing: Vec<_> = after.iter().filter(|name| before.contains(name)).collect();
    assert_eq!(existing, before.iter().collect::<Vec<_>>());
    let letters: Vec<&[u8]> = [b"a", b"b", b"c", b"d", b"e", b"f", b"g"]
        .iter()
        .map(|name| name.as_slice())
        .collect();
    assert_eq!(after, letters);
    Ok(())
}
