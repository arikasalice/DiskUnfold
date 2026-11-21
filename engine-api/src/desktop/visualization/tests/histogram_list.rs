//! The list condition of a histogram interval (#297): the files query.run finds with it
//! are exactly the interval's members, so the list count equals the bar count.
use super::*;
use crate::browse::folder::lower_condition;
use du_analysis_index::query::{self, Basis, Registry, Sort};
use du_core_contracts::{ItemRef, browse_open_ipc::QueryCondition};
use std::collections::BTreeSet;

/// Files on both sides of the SI interval bounds 1 kB and 1 MB.
fn boundaries() -> Publication<'static> {
    Publication {
        fingerprint: "scope-h",
        lineage_candidate: 1,
        scope: 2,
        revision: 3,
        files: &[
            (b'a', Some(5)),
            (b'b', Some(999)),
            (b'c', Some(1_000)),
            (b'd', Some(999_999)),
            (b'e', Some(1_000_000)),
        ],
        named: &[],
    }
}

fn item_nos(rows: &Value) -> Result<BTreeSet<u64>, String> {
    rows["rows"]
        .as_array()
        .ok_or("rows are not a list")?
        .iter()
        .map(|row| {
            row["itemRef"]["itemNo"]
                .as_u64()
                .ok_or_else(|| format!("{row}"))
        })
        .collect()
}

fn listed(harness: &Harness, condition: &Value) -> Result<BTreeSet<u64>, String> {
    let wire: QueryCondition =
        serde_json::from_value(condition.clone()).map_err(|error| error.to_string())?;
    let condition = lower_condition(wire).map_err(|error| format!("{error:?}"))?;
    let version = harness
        .sink
        .pin_current(&id(3))
        .map_err(|error| format!("{error:?}"))?;
    let sort = Sort {
        id: "sort.path",
        revision: 1,
        basis: Basis::Logical,
        descending: false,
    };
    let result = query::run(&Registry::baseline(), version, &condition, sort, || false)
        .map_err(|error| format!("{error:?}"))?;
    Ok(result
        .items
        .iter()
        .map(|item: &ItemRef| u64::from(item.item_no))
        .collect())
}

#[test]
fn an_interval_lists_exactly_its_bar_with_its_bounds_on_either_side() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&boundaries())?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "histogram", 3))?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let rows = rows_of(&harness, &answer)?;
    let mut checked = 0;
    for row in rows["rows"].as_array().ok_or("no rows")? {
        let Some(group) = row["groupRef"].as_str() else {
            continue;
        };
        let count = row["memberCount"]["value"]
            .as_str()
            .and_then(|count| count.parse::<usize>().ok())
            .ok_or_else(|| format!("no member count: {row}"))?;
        if count == 0 {
            continue;
        }
        let condition = &row["condition"];
        assert_eq!(condition["kind"], "all", "{row}");
        let members = harness.ok(
            "viz.group_rows",
            json!({
                "modelRef": answer["model"]["modelRef"], "groupRef": group,
                "start": 0, "count": 1000,
            }),
        )?;
        let bar = item_nos(&members)?;
        assert_eq!(bar.len(), count, "{row}");
        assert_eq!(listed(&harness, condition)?, bar, "{row}");
        checked += 1;
    }
    // 5 and 999 below 1 kB, 1 000 and 999 999 below 1 MB, 1 000 000 above.
    assert_eq!(checked, 3);
    Ok(())
}
