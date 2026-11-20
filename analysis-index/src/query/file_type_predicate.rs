//! The type predicate pred.file_type: a file whose type is the operand.
//!
//! Revision 1 reads the type classification table revision 1 (F04 FR14, shared
//! with the F06 type legend), so a file matches the legend row it is counted in.
//! The operand is the type key of one listed type (type.document ...
//! type.unclassified), as UTF-8 bytes. The class of no type ("unclassified" in a
//! legend) is not an operand: it has no condition.
use super::{Predicate, Record, Registry, Truth};
use crate::visualization::type_legend;
use du_core_contracts::result::ResultItemKind;

/// The registered predicate ID.
pub const ID: &str = "pred.file_type";
/// The revision, bound to type classification table revision 1.
pub const REVISION: u32 = 1;

/// Adds the predicate and its operand check to a baseline registry.
pub(super) fn register(registry: &mut Registry) {
    let key = (ID.to_owned(), REVISION);
    registry.predicates.insert(key.clone(), evaluate);
    registry.operand_validators.insert(key, valid_operand);
}

/// A file matches when its type is the operand. A folder or an untraversed link is
/// never a file of a type. An item of unknown kind, and a file whose name cannot be
/// read in its root's encoding, have an unknown type, which is not a mismatch.
pub(super) fn evaluate(record: &Record<'_>, predicate: &Predicate) -> Truth {
    let Predicate::Registered { operand, .. } = predicate else {
        return Truth::Unknown;
    };
    let Some(wanted) = type_legend::type_of_key(operand) else {
        return Truth::Unknown;
    };
    match record.item.kind {
        ResultItemKind::File => {
            match type_legend::file_type_of(record.version, record.item.item_no) {
                Some(kind) if kind == wanted => Truth::Match,
                Some(_) => Truth::NoMatch,
                None => Truth::Unknown,
            }
        }
        ResultItemKind::Directory | ResultItemKind::UntraversedLink => Truth::NoMatch,
        ResultItemKind::Unknown | ResultItemKind::UnknownStructure => Truth::Unknown,
    }
}

/// An operand is the key of one listed type.
pub(super) fn valid_operand(operand: &[u8]) -> bool {
    type_legend::type_of_key(operand).is_some()
}
