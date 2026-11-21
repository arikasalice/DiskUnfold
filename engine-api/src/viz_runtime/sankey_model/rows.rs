//! The node rows of a Sankey model and the sizes they carry (#296).
use super::Node;
use crate::{
    display_path,
    viz_runtime::{Admitted, EngineError, internal},
};
use du_analysis_index::visualization::{live_models::GroupRows, ratio, type_legend};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::ResultVersionRead,
    visualization::{
        item_kind::VizItemKind,
        quantity::VizQuantity,
        rows::{VizDenominator, VizRatio, VizRow},
        sankey::VizColumn,
    },
};

pub(super) fn group_rows(rows: Vec<VizRow>, basis: &SizeBasis) -> GroupRows {
    let total = rows.iter().filter_map(|row| match &row.size.amount {
        SizeAmount::Exact { bytes } => Some(bytes.get()),
        _ => None,
    });
    GroupRows {
        total: size(basis, total.sum()),
        rows,
        size_range: du_analysis_index::visualization::live_models::SizeRange::Unknown,
    }
}

pub(super) fn size(basis: &SizeBasis, bytes: u64) -> SizeValue {
    SizeValue {
        basis: basis.clone(),
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}

pub(super) fn bytes(value: u64) -> Result<VizQuantity, EngineError> {
    VizQuantity::exact("bytes", i128::from(value)).ok_or_else(internal)
}

/// Builds node rows against the scope total.
pub(super) struct Rows<'a> {
    pub(super) admitted: &'a Admitted,
    pub(super) denominator: &'a VizDenominator,
}

impl Rows<'_> {
    fn share(&self, value: &VizQuantity) -> Result<Option<VizRatio>, EngineError> {
        ratio::calculate(value, &self.denominator.value)
            .as_deref()
            .map(VizRatio::parse)
            .transpose()
            .map_err(|_| internal())
    }

    pub(super) fn item(
        &self,
        column: VizColumn,
        shape_ref: String,
        item_no: u32,
        value: u64,
        rect: Option<du_analysis_index::visualization::geometry::Rect>,
    ) -> Result<Node, EngineError> {
        let row = self.item_row(item_no, value)?;
        Ok(Node {
            column,
            shape_ref,
            row_ref: format!("item:{item_no}"),
            legend_class_key: type_legend::FOLDER.to_owned(),
            member_count: None,
            rect,
            row,
        })
    }

    /// A member of the Other folder node, listed through viz.group_rows.
    pub(super) fn member(&self, item_no: u32) -> Result<VizRow, EngineError> {
        let version = self.admitted.source.version();
        let value = exact(version.item_size(item_no, self.admitted.request.size_basis.clone()))
            .unwrap_or(0);
        self.item_row(item_no, value)
    }

    fn item_row(&self, item_no: u32, value: u64) -> Result<VizRow, EngineError> {
        let version = self.admitted.source.version();
        let reference = ItemRef {
            result_version: self.admitted.source.result_version().clone(),
            item_no,
        };
        let item = version.items().get(item_no as usize).ok_or_else(internal)?;
        let (_, root) = version
            .observed_root_for(&reference)
            .map_err(|_| internal())?;
        let display_name =
            display_path::name(&root.path.encoding, &item.name).ok_or_else(internal)?;
        let quantity = bytes(value)?;
        Ok(VizRow {
            item_ref: Some(reference),
            group_ref: None,
            display_name,
            size: size(&self.admitted.request.size_basis, value),
            quantity: None,
            ratio: self.share(&quantity)?,
            denominator: self.denominator.clone(),
            legend_class_key: type_legend::FOLDER.to_owned(),
            display_flags: vec![],
            label_key: None,
            member_count: None,
            item_kind: VizItemKind::from_result(item.kind),
            condition: None,
            unread: None,
            type_slices: None,
            links: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn group(
        &self,
        column: VizColumn,
        shape_ref: String,
        group_ref: &str,
        label_key: &str,
        class_key: &str,
        value: u64,
        rect: Option<du_analysis_index::visualization::geometry::Rect>,
    ) -> Result<Node, EngineError> {
        let quantity = bytes(value)?;
        let row = VizRow {
            item_ref: None,
            group_ref: Some(group_ref.to_owned()),
            display_name: label_key.to_owned(),
            size: size(&self.admitted.request.size_basis, value),
            ratio: self.share(&quantity)?,
            quantity: Some(quantity),
            denominator: self.denominator.clone(),
            legend_class_key: class_key.to_owned(),
            display_flags: vec![],
            label_key: Some(label_key.to_owned()),
            member_count: None,
            item_kind: None,
            condition: None,
            unread: None,
            type_slices: None,
            links: None,
        };
        Ok(Node {
            column,
            shape_ref,
            row_ref: format!("group:{group_ref}"),
            legend_class_key: class_key.to_owned(),
            member_count: None,
            rect,
            row,
        })
    }
}

/// The bytes of a size that is exact and known on its basis.
pub(in crate::viz_runtime) fn exact(size: Option<&SizeValue>) -> Option<u64> {
    match size {
        Some(size) if size.evidence != SizeEvidence::Unknown => match &size.amount {
            SizeAmount::Exact { bytes } => Some(bytes.get()),
            _ => None,
        },
        _ => None,
    }
}
