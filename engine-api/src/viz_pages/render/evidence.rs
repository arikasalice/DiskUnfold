//! Normalization, condition fingerprint and presentation of one render's child evidence.
use super::*;

pub(super) fn normalize(
    evidence: Vec<ChildEvidence>,
    revision: &Id,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<BTreeMap<u32, ChildEvidence>, Error> {
    let mut normalized = BTreeMap::new();
    for mut child in evidence {
        check(cancelled)?;
        child.display_flags.sort_unstable();
        child.display_flags.dedup();
        if &child.item.result_version != revision
            || normalized.insert(child.item.item_no, child).is_some()
        {
            return Err(Error::InvalidEvidence);
        }
    }
    Ok(normalized)
}

pub(super) fn fingerprint(
    request: &Request<'_>,
    strategy: &LayoutStrategyKey,
    evidence: &BTreeMap<u32, ChildEvidence>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<String, Error> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"diskunfold.live-viz.unfiltered.v1\0");
    let bounds = request.bounds;
    let envelope = (
        request.parent,
        &request.basis,
        [bounds.left, bounds.top, bounds.right, bounds.bottom],
        request.format,
        (&strategy.id, strategy.revision),
        request.max_shapes,
        request.other.map(|other| {
            (
                &other.group_ref,
                &other.display_name,
                &other.legend_class_key,
                &other.display_flags,
                &other.label_key,
            )
        }),
    );
    serde_json::to_writer(&mut hash, &envelope).map_err(|_| Error::Fingerprint)?;
    for child in evidence.values() {
        check(cancelled)?;
        hash.update(b"\n");
        let row = (
            &child.item,
            &child.path_compare_key,
            &child.legend_class_key,
            &child.display_flags,
        );
        serde_json::to_writer(&mut hash, &row).map_err(|_| Error::Fingerprint)?;
    }
    Ok(hash.finalize().to_hex().to_string())
}

pub(super) fn presentation(child: ChildEvidence) -> RowPresentation {
    RowPresentation {
        item: child.item,
        legend_class_key: child.legend_class_key,
        display_flags: child.display_flags,
    }
}

pub(super) fn check(cancelled: &mut dyn FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
