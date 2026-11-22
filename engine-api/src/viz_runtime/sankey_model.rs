//! The Sankey model and its node rows (#296, F06 README 12.1): one function turns the
//! laid-out graph into shapes, links and rows, so a link's relationship and the row
//! that lists it carry the same bytes.
use super::*;
use crate::{viz_pages::ModelInput, viz_production::policy};
use du_analysis_index::visualization::{
    sankey::geometry_v1::{Endpoint, Relations},
    sankey::{CategoryKey, FolderKey, Graph},
    type_legend,
    typed_layout::SankeyOutput,
};
use du_core_contracts::{
    SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    browse_page_ipc::{QueryStrategy, ResultOrigin},
    result::ResultVersionRead,
    visualization::{
        geometry::{Geometry, Point},
        model::{
            VizMatchKey, VizRelationship, VizRepresentation, VizShape, VizSummary, VizTotal,
            VizTotals,
        },
        quantity::VizQuantity,
        rows::{VizDenominator, VizDenominatorBasis, VizRow},
        sankey::{
            SANKEY_LINK_KIND, SANKEY_TYPE_REF_PREFIX, SANKEY_UNATTRIBUTED_LABEL_KEY,
            SANKEY_UNATTRIBUTED_REF, VizColumn, VizLink,
        },
        type_slices::OTHER_GROUP_LABEL_KEY,
    },
};
use std::collections::HashMap;

/// The model-local group reference of the Other folder node.
const OTHER_REF: &str = "viz.sankey.other";

mod rows;
pub(super) use rows::exact;
use rows::{Rows, bytes, group_rows, size};

/// One node: its column, shape reference, row and geometry.
struct Node {
    column: VizColumn,
    shape_ref: String,
    row_ref: String,
    row: VizRow,
    legend_class_key: String,
    member_count: Option<VizQuantity>,
    rect: Option<du_analysis_index::visualization::geometry::Rect>,
}

pub(super) fn build(
    admitted: &Admitted,
    output: &SankeyOutput,
    scope_bytes: u64,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(ModelInput, VizModel), EngineError> {
    let request = &admitted.request;
    let version = admitted.source.version();
    let graph = &output.graph;
    let scope_size = size(&request.size_basis, scope_bytes);
    let scope_total = VizTotal {
        quantity: bytes(scope_bytes)?,
        source_size: Some(scope_size.clone()),
    };
    let denominator = VizDenominator {
        basis: VizDenominatorBasis::Scope,
        value: scope_total.quantity.clone(),
        size: Some(scope_size),
    };
    let rows = Rows {
        admitted,
        denominator: &denominator,
    };
    // Nodes in table order: the scope, the folders, the types (README 12.1).
    let mut nodes = vec![rows.item(
        VizColumn::Scope,
        "sankey.scope".into(),
        request.aggregate_scope_ref.item_no,
        graph.volume.bytes,
        graph.volume.rect,
    )?];
    let mut groups = HashMap::new();
    let mut other_members = 0;
    for folder in &graph.folders {
        if cancelled() {
            return Err(internal());
        }
        nodes.push(match folder.key {
            FolderKey::Folder(item_no) => rows.item(
                VizColumn::Folder,
                folder_shape(folder.key),
                item_no,
                folder.bytes,
                folder.rect,
            )?,
            FolderKey::Other => {
                let members = folder
                    .members
                    .iter()
                    .map(|item_no| rows.member(*item_no))
                    .collect::<Result<Vec<_>, _>>()?;
                other_members = members.len() as u64;
                groups.insert(
                    OTHER_REF.to_owned(),
                    group_rows(members, &request.size_basis),
                );
                let mut node = rows.group(
                    VizColumn::Folder,
                    folder_shape(folder.key),
                    OTHER_REF,
                    OTHER_GROUP_LABEL_KEY,
                    type_legend::FOLDER,
                    folder.bytes,
                    folder.rect,
                )?;
                let count =
                    VizQuantity::exact("items", i128::from(other_members)).ok_or_else(internal)?;
                node.row.member_count = Some(count.clone());
                node.member_count = Some(count);
                node
            }
            FolderKey::Unattributed => {
                groups.insert(
                    SANKEY_UNATTRIBUTED_REF.to_owned(),
                    group_rows(vec![], &request.size_basis),
                );
                rows.group(
                    VizColumn::Folder,
                    folder_shape(folder.key),
                    SANKEY_UNATTRIBUTED_REF,
                    SANKEY_UNATTRIBUTED_LABEL_KEY,
                    type_legend::FOLDER,
                    folder.bytes,
                    folder.rect,
                )?
            }
        });
    }
    for category in &graph.categories {
        let (group_ref, label_key, class_key) = category_identity(category.key);
        groups.insert(group_ref.clone(), group_rows(vec![], &request.size_basis));
        nodes.push(rows.group(
            VizColumn::Type,
            type_shape(category.key),
            &group_ref,
            label_key,
            class_key,
            category.bytes,
            category.rect,
        )?);
    }
    attach_links(graph, &mut nodes)?;
    let (shapes, relationships) = drawing(&nodes, &output.relations, &request.size_basis)?;
    let drawn: u64 = graph
        .folders
        .iter()
        .filter(|folder| matches!(folder.key, FolderKey::Folder(_)))
        .map(|folder| folder.bytes)
        .sum();
    let derived = |value: u64| -> Result<VizTotal, EngineError> {
        Ok(VizTotal {
            quantity: bytes(value)?,
            source_size: Some(size(&request.size_basis, value)),
        })
    };
    let totals = VizTotals {
        drawn: derived(drawn)?,
        grouped: derived(scope_bytes - drawn)?,
        filtered_out: derived(0)?,
        remainder: Some(derived(0)?),
        remainder_reason_keys: None,
        scope_measurement_limits: crate::space::item_limits(
            version,
            request.aggregate_scope_ref.item_no,
            &request.size_basis,
        ),
        scope: scope_total,
    };
    let model = VizModel {
        model_ref: "pending".into(),
        condition_fingerprint: admitted.key.clone(),
        aggregate_scope_ref: request.aggregate_scope_ref.clone(),
        navigation_scope_ref: request.navigation_scope_ref.clone(),
        focus_item_ref: request.focus_item_ref.clone(),
        origin: ResultOrigin("scan".into()),
        data_source: Some(version.data_source()),
        staleness: admitted.facts.staleness.clone(),
        representation: VizRepresentation {
            format: request.format.clone(),
            measure: request.measure.clone(),
            size_basis: request.size_basis.clone(),
            classification_criterion: request.classification_criterion.clone(),
            color_criterion: request.color_criterion.clone(),
            denominator_basis: request.denominator_basis,
            color_mode: None,
            label_mode: None,
        },
        denominator,
        totals,
        shapes,
        relationships,
        legend: legend(admitted, cancelled)?,
        summary: VizSummary {
            grouped_items: VizQuantity::exact("items", i128::from(other_members))
                .ok_or_else(internal)?,
            omitted_items: VizQuantity::exact("items", 0).ok_or_else(internal)?,
            unknown_items: VizQuantity::exact("items", 0).ok_or_else(internal)?,
            reason_keys: vec![],
            unknown_size: None,
            first_unknown_row: None,
            unread_folders: None,
            first_unread_row: None,
        },
        strategy: QueryStrategy {
            id: admitted.descriptor.strategy.id.clone(),
            revision: admitted.descriptor.strategy.revision,
        },
        table_revisions: admitted.facts.tables.clone(),
    };
    model
        .validate_revision(admitted.source.result_version())
        .map_err(|_| internal())?;
    let input = ModelInput {
        version: version.clone(),
        size_basis: admitted.request.size_basis.clone(),
        condition_fingerprint: admitted.key.clone(),
        rows: nodes.into_iter().map(|node| node.row).collect(),
        groups,
    };
    Ok((input, model))
}

/// The links leaving each folder node, folder to type, in the type column's order.
/// The scope's links are the folder nodes themselves, so its row lists none.
fn attach_links(graph: &Graph, nodes: &mut [Node]) -> Result<(), EngineError> {
    let type_rows: HashMap<String, String> = nodes
        .iter()
        .filter(|node| node.column == VizColumn::Type)
        .map(|node| (node.shape_ref.clone(), node.row_ref.clone()))
        .collect();
    for node in nodes
        .iter_mut()
        .filter(|node| node.column == VizColumn::Folder)
    {
        let mut links = Vec::new();
        for category in &graph.categories {
            let Some(link) = graph.links.iter().find(|link| {
                folder_shape(link.folder) == node.shape_ref && link.category == category.key
            }) else {
                continue;
            };
            if link.bytes == 0 {
                continue;
            }
            let to_row_ref = type_rows
                .get(&type_shape(category.key))
                .cloned()
                .ok_or_else(internal)?;
            links.push(VizLink {
                to_row_ref,
                quantity: bytes(link.bytes)?,
            });
        }
        if !links.is_empty() {
            node.row.links = Some(links);
        }
    }
    Ok(())
}

/// Node shapes with a rectangle, and the positive links between drawn nodes.
fn drawing(
    nodes: &[Node],
    relations: &Relations,
    basis: &SizeBasis,
) -> Result<(Vec<VizShape>, Vec<VizRelationship>), EngineError> {
    let mut shapes = Vec::new();
    for node in nodes {
        let Some(rect) = node.rect else { continue };
        let geometry = Geometry::new(
            "rect",
            vec![
                Point {
                    x: rect.left,
                    y: rect.top,
                },
                Point {
                    x: rect.right,
                    y: rect.bottom,
                },
            ],
        )
        .map_err(|_| internal())?;
        shapes.push(VizShape {
            item_ref: node.row.item_ref.clone(),
            group_ref: node.row.group_ref.clone(),
            match_key: if node.row.item_ref.is_some() {
                VizMatchKey::Unknown {
                    reason_key: policy::MATCH_KEY_REASON.to_owned(),
                }
            } else {
                VizMatchKey::NotApplicable {}
            },
            geometry,
            size: node.row.size.clone(),
            quantity: None,
            legend_class_key: node.legend_class_key.clone(),
            display_flags: vec![],
            shape_ref: Some(node.shape_ref.clone()),
            parent_shape_ref: None,
            visible_depth: None,
            root_branch_ref: None,
            member_count: node.member_count.clone(),
            item_kind: node.row.item_kind,
            type_slices: None,
            column: Some(node.column),
        });
    }
    let drawn: std::collections::HashSet<&str> = shapes
        .iter()
        .filter_map(|shape| shape.shape_ref.as_deref())
        .collect();
    let mut relationships = Vec::new();
    for relation in relations
        .volume_to_folder
        .iter()
        .chain(&relations.folder_to_category)
    {
        let (Some(geometry), from, to) = (
            relation.geometry.clone(),
            endpoint_shape(relation.from),
            endpoint_shape(relation.to),
        ) else {
            continue;
        };
        if relation.bytes == 0 || !drawn.contains(from.as_str()) || !drawn.contains(to.as_str()) {
            continue;
        }
        relationships.push(VizRelationship {
            kind: SANKEY_LINK_KIND.to_owned(),
            from_shape_ref: from,
            to_shape_ref: to,
            geometry: Some(geometry),
            quantity: Some(bytes(relation.bytes)?),
        });
    }
    let _ = basis;
    Ok((shapes, relationships))
}

fn endpoint_shape(endpoint: Endpoint) -> String {
    match endpoint {
        Endpoint::Volume => "sankey.scope".into(),
        Endpoint::Folder(key) => folder_shape(key),
        Endpoint::Category(key) => type_shape(key),
    }
}

fn folder_shape(key: FolderKey) -> String {
    match key {
        FolderKey::Folder(item_no) => format!("sankey.folder.{item_no}"),
        FolderKey::Other => "sankey.folder.other".into(),
        FolderKey::Unattributed => "sankey.folder.unattributed".into(),
    }
}

fn type_shape(key: CategoryKey) -> String {
    match key {
        CategoryKey::Category(index) => format!("sankey.type.{index}"),
        CategoryKey::Unattributed => "sankey.type.unattributed".into(),
    }
}

/// A type node's group reference, label key and legend class.
fn category_identity(key: CategoryKey) -> (String, &'static str, &'static str) {
    match key {
        CategoryKey::Category(index) => {
            let class = &type_legend::LISTED[(index as usize).min(type_legend::LISTED.len() - 1)];
            (
                format!("{SANKEY_TYPE_REF_PREFIX}{}", class.key),
                class.label_key,
                class.key,
            )
        }
        CategoryKey::Unattributed => (
            format!("{SANKEY_TYPE_REF_PREFIX}unattributed"),
            SANKEY_UNATTRIBUTED_LABEL_KEY,
            type_legend::NO_CLASS,
        ),
    }
}

/// The type legend of the scope, as the other formats count it.
fn legend(
    admitted: &Admitted,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<du_core_contracts::visualization::model::VizLegendEntry>, EngineError> {
    let request = &admitted.request;
    let totals = type_legend::scope_totals(
        admitted.source.version(),
        &request.aggregate_scope_ref,
        &request.size_basis,
        cancelled,
    )
    .map_err(|_| internal())?;
    let mut legend = Vec::new();
    for class in type_legend::LISTED {
        let Some(total) = totals.get(class.key) else {
            continue;
        };
        legend.push(du_core_contracts::visualization::model::VizLegendEntry {
            key: class.key.to_owned(),
            label_key: class.label_key.to_owned(),
            item_count: VizQuantity::exact("items", i128::from(total.count))
                .ok_or_else(internal)?,
            total: VizTotal {
                quantity: if total.lower_bound {
                    VizQuantity::lower_bound("bytes", total.bytes)
                } else {
                    VizQuantity::exact("bytes", i128::from(total.bytes))
                }
                .ok_or_else(internal)?,
                source_size: Some(SizeValue {
                    basis: request.size_basis.clone(),
                    evidence: SizeEvidence::Measured,
                    amount: if total.lower_bound {
                        SizeAmount::LowerBound {
                            bytes: total.bytes.into(),
                        }
                    } else {
                        SizeAmount::Exact {
                            bytes: total.bytes.into(),
                        }
                    },
                }),
            },
            condition: crate::viz_runtime::type_fragment(class.key),
        });
    }
    Ok(legend)
}
