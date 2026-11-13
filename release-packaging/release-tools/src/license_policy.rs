//! SPDX license policy shared by the npm and notices checks.

use spdx::{AdditionItem, Expression, LicenseItem};
use std::collections::BTreeSet;

/// Returns true when the expression is fully known and at least one branch
/// consists only of allowed licenses. Unknown identifiers, or-later forms and
/// custom additions are never permitted.
pub fn permits(expression: &str, allowed: &BTreeSet<String>) -> bool {
    let Ok(expression) = Expression::parse(expression) else {
        return false;
    };
    let known = expression.requirements().all(|item| {
        matches!(
            item.req.license,
            LicenseItem::Spdx {
                or_later: false,
                ..
            }
        ) && !matches!(item.req.addition, Some(AdditionItem::Other(_)))
    });
    known && expression.evaluate(|req| allowed.contains(&req.to_string()))
}
