use du_release_tools::license_policy::permits;
use serde::Deserialize;
use std::{collections::BTreeSet, io};

#[derive(Deserialize)]
struct Check {
    expression: String,
    allowed: BTreeSet<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let checks: Vec<Check> = serde_json::from_reader(io::stdin().lock())?;
    let decisions: Vec<_> = checks
        .iter()
        .map(|check| permits(&check.expression, &check.allowed))
        .collect();
    serde_json::to_writer(io::stdout().lock(), &decisions)?;
    Ok(())
}
