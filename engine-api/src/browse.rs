//! Projects pinned item observations into the main-window detail contract.
pub mod copy_path;
pub mod folder;
use du_analysis_index::{IngestSink, QueryError, ResultVersion};
use du_core_contracts::{
    PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    browse_ipc::{BrowseDetail, BrowseDetailRequest, BrowseOrigin, BrowseValue},
    result::{ResultItem, ResultItemKind, ResultVersionRead},
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

/// Registers local detail reads against the live scan publication point.
pub(crate) fn register(builder: &mut EngineBuilder, store: IngestSink) {
    builder.register(Detail(store));
}

struct Detail(IngestSink);

impl UseCase for Detail {
    const NAME: &'static str = "browse.detail";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = BrowseDetailRequest;
    type Response = BrowseDetail;

    fn check_scope(&self, request: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        if request.result_version != request.item.result_version {
            return Err(EngineError::reason("result.requery_required"));
        }
        Ok(())
    }

    fn execute(&self, request: Self::Request, _: &CallerCtx) -> Result<BrowseDetail, EngineError> {
        let version = self
            .0
            .pin_current(&request.result_version)
            .map_err(reason)?;
        project(&version, request)
    }
}

fn project(
    version: &ResultVersion,
    request: BrowseDetailRequest,
) -> Result<BrowseDetail, EngineError> {
    let detail = version.detail(&request.item).map_err(reason)?;
    let item_no = detail.item.item_no;
    let display_path = version.native_path(&request.item).ok().and_then(|path| {
        let bytes = path.raw.bytes().ok()?;
        display(&path.encoding, &bytes)
    });
    let display_name = match detail.item.parent {
        None => display_path.clone(),
        Some(_) => item_display_name(version, &request.item, detail.item),
    };
    let kind = match detail.item.kind {
        ResultItemKind::Unknown | ResultItemKind::UnknownStructure => unknown("ref.unavailable"),
        kind => BrowseValue::Present { value: kind },
    };
    Ok(BrowseDetail {
        item: request.item,
        display_path: field(display_path, "ref.unavailable"),
        display_name: field(display_name, "ref.unavailable"),
        kind,
        logical_size: size(version.item_size(item_no, SizeBasis::Logical)),
        allocated_size: size(version.item_size(item_no, SizeBasis::Allocated)),
        modified_at: field(
            version.item_modified_at(item_no).cloned(),
            "ref.unavailable",
        ),
        created_at: unknown("ref.unavailable"),
        observed_at: field(version.observed_at().cloned(), "ref.unavailable"),
        origin: BrowseOrigin::Scan,
        data_source: Some(version.data_source()),
        completeness: version.completeness(),
        // The current scan model cannot establish a live correspondence from observation alone.
        current_correspondence: false,
        observation_only: true,
    })
}

#[cfg(test)]
mod tests;

fn reason(error: QueryError) -> EngineError {
    EngineError::reason(match error {
        QueryError::StaleVersion => "result.requery_required",
        QueryError::Unavailable => "support.internal_error",
        _ => "ref.unavailable",
    })
}

fn field<T>(value: Option<T>, reason_key: &str) -> BrowseValue<T> {
    value.map_or_else(
        || unknown(reason_key),
        |value| BrowseValue::Present { value },
    )
}

fn unknown<T>(reason_key: &str) -> BrowseValue<T> {
    BrowseValue::Unknown {
        reason_key: reason_key.into(),
    }
}

fn size(value: Option<&SizeValue>) -> BrowseValue<SizeValue> {
    field(
        value
            .filter(|size| {
                size.amount != SizeAmount::Unknown && size.evidence != SizeEvidence::Unknown
            })
            .cloned(),
        "space.data_insufficient",
    )
}

fn item_display_name(
    version: &ResultVersion,
    reference: &du_core_contracts::ItemRef,
    item: &ResultItem,
) -> Option<String> {
    let (_, root) = version.observed_root_for(reference).ok()?;
    if item.parent.is_none() {
        return display(&root.path.encoding, &root.path.raw.bytes().ok()?);
    }
    display(&root.path.encoding, &item.name)
}

pub(crate) fn display(encoding: &PathEncoding, bytes: &[u8]) -> Option<String> {
    match encoding {
        PathEncoding::Unix => Some(String::from_utf8_lossy(bytes).into_owned()),
        PathEncoding::WindowsUtf16le => {
            let (pairs, remainder) = bytes.as_chunks::<2>();
            if !remainder.is_empty() {
                return None;
            }
            let units: Vec<_> = pairs.iter().copied().map(u16::from_le_bytes).collect();
            Some(String::from_utf16_lossy(&units))
        }
    }
}
