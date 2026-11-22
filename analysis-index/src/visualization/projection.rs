//! Pinned, level-of-detail visualization projection for the admitted layout boundary.
//!
//! This is an internal projection, not the public F06 response: callers must
//! supply policy, provenance, model identity, paging and IPC contracts separately.
use super::{
    filter::ItemFilter,
    geometry::{NoArea, Rect},
    live_models::SizeRange,
    model_links::ModelLinkError,
    typed_layout::{self, Dispatcher},
};
use crate::ResultVersion;
use du_core_contracts::{
    Id, ItemRef, SizeBasis, SizeValue, Timestamp,
    result::{ResultCompleteness, ResultVersionRead},
    visualization::{
        LayoutDescriptor, LayoutStrategyKey,
        geometry::Geometry,
        model::{ShapeMeta, VizRelationship},
    },
};

/// An original F03 row shared by the drawn shape and fallback list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub item: ItemRef,
    pub size: SizeValue,
    /// First child folder of the navigation root, or no established branch.
    pub root_branch_ref: Option<ItemRef>,
    /// Source value state, independent of whether the row has geometry.
    pub area_value: super::AreaValue,
    /// Layout omission reason, or the reason no area could be established. A row
    /// without a shape of its own may also have none: see [lod](Self::lod).
    pub omission: Option<NoArea>,
    /// Immediate children outside this single-level projection, not a recursive count.
    pub omitted_children: usize,
    /// Ratio to the admitted parent's exact value in the selected size basis.
    pub ratio: Option<String>,
    /// How the level-of-detail policy represents this row.
    pub lod: Lod,
}

/// How the level-of-detail policy represents a sibling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lod {
    /// The strategy decided: the row has a shape, or its omission says why not.
    Drawn,
    /// A member of the group at this index of [Detail::groups].
    Grouped(usize),
    /// Below the minimum area with no group to join; counted but not drawn.
    Omitted,
}

/// A format-independent shape referring to an entry in rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub row_index: usize,
    pub meta: ShapeMeta,
    pub geometry: Geometry,
}
/// A descendant shape backed by its own pinned observation, separate from table rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescendantShape {
    pub item: ItemRef,
    pub size: SizeValue,
    pub meta: ShapeMeta,
    pub geometry: Geometry,
}
/// One member of a group, with its original F03 value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupMember {
    pub item: ItemRef,
    pub size: SizeValue,
}
/// The small siblings of one parent merged into a single shape.
///
/// Members keep the sibling order of the admission, the same order the table uses and
/// the one viz.group_rows pages through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub parent: ItemRef,
    /// Opaque reference valid only inside this model; one group exists per parent.
    pub group_ref: String,
    pub meta: ShapeMeta,
    pub geometry: Geometry,
    pub members: Vec<GroupMember>,
    /// Sum of the attributed members; a lower bound whenever a member is.
    pub total: SizeValue,
    /// Known only when every member is an exact value in the model's size basis.
    pub size_range: SizeRange,
    /// For the navigation-root group: retained rows listed before it in table order.
    pub slot: usize,
}
/// Level-of-detail results that have no place in the row table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detail {
    /// Smallest shape area used for every level of this model; zero when not applied.
    pub min_area: u64,
    /// Whether the small siblings of any parent, including expanded folders, may
    /// merge into a group.
    pub group_tail: bool,
    /// Groups of every drawn level; the navigation-root group, when any, is first.
    pub groups: Vec<Group>,
    /// Children without a provable area, at every drawn level.
    pub unknown_items: u64,
    /// The navigation-root share that no sibling could take as area (F06 README 51.3).
    pub unknown_share: UnknownShare,
    /// Row of the first navigation-root sibling without a provable area, or else of the
    /// first expanded folder whose own level has one; none without such a child.
    pub first_unknown_row: Option<usize>,
    /// Navigation-root siblings with neither a shape nor a group. Hidden children of
    /// drawn folders are counted by [Row::omitted_children] instead.
    pub omitted_items: u64,
    /// Reason keys the collector gave for unread ranges inside the scope, sorted.
    pub reason_keys: Vec<String>,
    /// What the remainder of [Totals] holds.
    pub remainder_causes: RemainderCauses,
}
/// Bytes of the scope measured under navigation-root siblings without a provable area
/// and outside every attributed sibling.
///
/// The scope's F03 value counts each measured file once, and the attributed siblings
/// hold disjoint measured parts of it, so the scope's partition bytes less theirs is
/// exactly the set of measured bytes under the other siblings. Expanded folders keep
/// their unattributed share inside their own drawn value, so it is not here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnknownShare {
    /// No navigation-root sibling lacks an area, or they hold no measured byte.
    #[default]
    None,
    Exact(u64),
    /// An unattributed sibling may hold an entry that could not be read.
    LowerBound(u64),
    /// A filter keeps a share of the unattributed siblings that cannot be proven.
    Unknown,
}
/// What the remainder of [Totals] holds, as facts about the navigation-root siblings.
///
/// The flags say what the remainder can be made of, not how much: a producer reads them
/// only when the remainder is not zero. Nested levels are not in them, because what a
/// folder's own level leaves out lies inside the folder's value, which a total adds once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemainderCauses {
    /// A sibling with bytes has neither a shape nor a group, because it is below the
    /// minimum area or lacks coordinate precision.
    pub too_small: bool,
    /// A sibling has no provable area: its value stays on its row and is not added to a
    /// total, because it shares a file with another sibling or has no value of its own.
    /// The scope counts what siblings share once, so no sibling carries that share.
    pub unattributed: bool,
}
/// The four byte totals of this single-parent projection.
///
/// They have one definition for every consumer:
/// - scope is the original F03 aggregate of the parent, never derived from geometry;
/// - drawn adds the navigation-root siblings that have a shape of their own. A group
///   shape is not in it, and nested shapes lie inside their folder's bytes, so they
///   are never added again;
/// - grouped adds the members of the navigation-root group, and nothing else. The
///   members of a nested group lie inside the folder that holds them;
/// - filtered adds what a filter removed from the attributed navigation-root siblings,
///   and is zero without a filter. Drawn and grouped then hold only kept bytes.
///
/// Drawn and grouped add disjoint sets of root siblings, so with exact values
/// drawn + grouped + filtered never exceeds scope. What none of them holds,
/// scope − drawn − grouped − filtered, is made of two things: siblings left out because
/// they are below the minimum area or lack coordinate precision, and the share of the
/// scope that no single sibling can carry, for example a file contained by two
/// siblings, which the scope counts once. The values of siblings without a provable
/// area stay on their rows and are never added to a total.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Totals {
    /// Original F03 set aggregate, never derived from geometry.
    pub scope: SizeValue,
    /// Outermost individually drawn siblings; group shapes and nested shapes are not in it.
    pub drawn_bytes: u64,
    /// Navigation-root group members only; nested groups lie inside a drawn shape's bytes.
    pub grouped_bytes: u64,
    pub filtered_bytes: u64,
    /// True when a summed row is a lower bound, so the sum is one too.
    pub drawn_lower_bound: bool,
    pub grouped_lower_bound: bool,
}
/// Immutable projection; it does not assign a public model reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub result_revision: Id,
    pub parent: ItemRef,
    pub basis: SizeBasis,
    pub completeness: ResultCompleteness,
    pub observed_at: Option<Timestamp>,
    pub strategy: LayoutDescriptor,
    pub rows: Vec<Row>,
    pub shapes: Vec<Shape>,
    pub descendants: Vec<DescendantShape>,
    pub relationships: Vec<VizRelationship>,
    pub omissions: Vec<typed_layout::Omission>,
    pub totals: Totals,
    /// The filter every level of the model applies; expanded folders apply it too.
    pub filter: Option<ItemFilter>,
    pub detail: Detail,
}

/// An observed item's location relative to this projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// Zero-based position in the model's geometry array.
    Shape(usize),
    /// A member of the group with this model-local reference.
    Group(String),
    /// Inside the pinned folder scope, with no geometry in this view.
    OutsideView,
    /// Observed in the same result, outside the pinned folder scope.
    OutsideScope,
}

impl Model {
    /// Number of shapes at every depth, groups included; the unit of the shape limit.
    pub fn shape_count(&self) -> usize {
        self.shapes
            .len()
            .saturating_add(self.descendants.len())
            .saturating_add(self.detail.groups.len())
    }

    /// Position of a row in the table of the navigation root, where the members of the
    /// root group leave the table and one group row stands at its slot. None for a
    /// member of that group or an unknown row.
    pub fn table_position(&self, row_index: usize) -> Option<usize> {
        let grouped = |row: &Row| matches!(row.lod, Lod::Grouped(_));
        if grouped(self.rows.get(row_index)?) {
            return None;
        }
        let before = self.rows[..row_index]
            .iter()
            .filter(|row| !grouped(row))
            .count();
        let group_before = self
            .detail
            .groups
            .first()
            .is_some_and(|group| group.parent == self.parent && group.slot <= before);
        Some(before + usize::from(group_before))
    }

    /// Locates an exact item reference using only the pinned hierarchy and model.
    ///
    /// # Errors
    /// Returns StaleVersion for another revision and NotFound for an unknown item.
    pub fn locate(
        &self,
        version: &ResultVersion,
        item: &ItemRef,
    ) -> Result<Location, crate::QueryError> {
        if version.version_id() != &self.result_revision {
            return Err(crate::QueryError::StaleVersion);
        }
        let item_no = version.detail(item)?.item.item_no;
        if let Some(index) = self.shapes.iter().position(|shape| {
            self.rows
                .get(shape.row_index)
                .is_some_and(|row| row.item == *item)
        }) {
            return Ok(Location::Shape(index));
        }
        if let Some(index) = self
            .descendants
            .iter()
            .position(|shape| shape.item == *item)
        {
            return Ok(Location::Shape(self.shapes.len() + index));
        }
        if let Some(group) = self
            .detail
            .groups
            .iter()
            .find(|group| group.members.iter().any(|member| member.item == *item))
        {
            return Ok(Location::Group(group.group_ref.clone()));
        }
        Ok(
            if super::descendant(version.items(), item_no, self.parent.item_no) {
                Location::OutsideView
            } else {
                Location::OutsideScope
            },
        )
    }
}
/// Locates an item relative to a pinned scope with no available shapes.
///
/// Both references must belong to the supplied result. In-scope items remain
/// available for table navigation even when there is no geometry to select.
pub fn locate_without_shapes(
    version: &ResultVersion,
    scope: &ItemRef,
    item: &ItemRef,
) -> Result<Location, crate::QueryError> {
    let scope = version.detail(scope)?.item.item_no;
    let item = version.detail(item)?.item.item_no;
    Ok(if super::descendant(version.items(), item, scope) {
        Location::OutsideView
    } else {
        Location::OutsideScope
    })
}

/// No model is produced on failure or cancellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidSource,
    UnavailableAttribution,
    Dispatch(typed_layout::DispatchError),
    InvalidLayout,
    Links(ModelLinkError),
    Cancelled,
}

/// Selection and view limit supplied by the caller.
pub struct Options<'a> {
    pub format: &'a str,
    pub strategy: Option<&'a LayoutStrategyKey>,
    /// Upper bound on shapes at every depth, groups included. It also sets the
    /// smallest shape area: the bounds divided by this limit.
    pub max_shapes: usize,
    /// Whether the small siblings of a parent may merge into one group. When false
    /// they are only counted, so the model needs no group presentation.
    pub group_tail: bool,
}

/// One pinned source.
pub struct Source<'a> {
    pub version: &'a ResultVersion,
    pub parent: &'a ItemRef,
    pub basis: SizeBasis,
    pub bounds: Rect,
}

/// Admits and projects rows once from the same immutable version and parent.
///
/// Scope is exactly the parent, with no filtering. Siblings whose area would be below
/// the minimum merge into one group, or are only counted when even the group is below
/// it, so the shape limit is never an error. Children without a provable area stay in
/// the rows without a shape. The projection fails only when the parent itself cannot be
/// partitioned. Consumers must not treat it as a complete public render response.
pub fn project(
    source: Source<'_>,
    dispatcher: &Dispatcher,
    options: Options<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Model, Error> {
    root::build(source, dispatcher, options, None, cancelled, true)
}

/// Builds an unpublished candidate whose links must be checked after adding branches.
pub fn project_deferred(
    source: Source<'_>,
    dispatcher: &Dispatcher,
    options: Options<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Model, Error> {
    root::build(source, dispatcher, options, None, cancelled, false)
}

/// Projects only what the filter keeps, as [project] does, or as [project_deferred]
/// when deferred. The scope total stays the parent's original value; the filtered
/// total says what the filter removed, and expanded folders apply it too.
pub fn project_filtered(
    source: Source<'_>,
    dispatcher: &Dispatcher,
    options: Options<'_>,
    filter: Option<ItemFilter>,
    deferred: bool,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Model, Error> {
    root::build(source, dispatcher, options, filter, cancelled, !deferred)
}

mod gaps;
mod hierarchy;
mod level;
mod root;
pub use hierarchy::{
    FolderFrame, add_child_partition, add_framed_child_partition, finish_child_partitions,
};

/// The model-local reference of the group that merges the small children of a parent.
fn group_ref(parent: &ItemRef) -> String {
    format!("g{:x}", parent.item_no)
}
