use serde::Deserialize;
use spdx::{AdditionItem, Expression, LicenseItem};
use std::{collections::BTreeSet, io};

#[derive(Deserialize)]
struct Check {
    expression: String,
    allowed: BTreeSet<String>,
}

fn permits(check: &Check) -> bool {
    let Ok(expression) = Expression::parse(&check.expression) else {
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
    known && expression.evaluate(|req| check.allowed.contains(&req.to_string()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let checks: Vec<Check> = serde_json::from_reader(io::stdin().lock())?;
    let decisions: Vec<_> = checks.iter().map(permits).collect();
    serde_json::to_writer(io::stdout().lock(), &decisions)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_known_expressions_and_an_allowed_branch() {
        for (expression, expected) in [
            ("MIT OR GPL-3.0-only", true),
            ("MIT AND GPL-3.0-only", false),
            ("MIT OR LicenseRef-unknown", false),
            ("MIT OR typo", false),
            ("MIT*", false),
            ("Apache-2.0 WITH LLVM-exception", true),
        ] {
            assert_eq!(
                permits(&Check {
                    expression: expression.into(),
                    allowed: ["MIT".into(), "Apache-2.0 WITH LLVM-exception".into()].into(),
                }),
                expected,
                "{expression}"
            );
        }
    }
}
