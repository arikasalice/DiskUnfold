use du_core_contracts::visualization::quantity::{VizAmount, VizQuantity};
use serde_json::{from_value, json, to_value};

#[test]
fn preserves_counts_sizes_signed_changes_and_missing_values() -> Result<(), serde_json::Error> {
    for value in [
        json!({"unit": "items", "state": "exact", "value": "2"}),
        json!({"unit": "bytes", "state": "exact", "value": "20"}),
        json!({"unit": "bytes", "state": "exact", "value": "-5"}),
        json!({"unit": "bytes", "state": "exact", "value": "18446744073709551615"}),
        json!({"unit": "bytes", "state": "exact", "value": "-18446744073709551615"}),
        json!({"unit": "bytes", "state": "unknown", "reasonKey": "space.data_insufficient"}),
        json!({"unit": "bytes", "state": "lower_bound", "value": "0"}),
        json!({"unit": "future_unit", "state": "exact", "value": "35"}),
    ] {
        let quantity: VizQuantity = from_value(value.clone())?;
        assert_eq!(to_value(quantity)?, value);
    }
    let delta = VizQuantity::exact("bytes", -5).ok_or_else(|| {
        serde_json::Error::io(std::io::Error::other("valid signed quantity was rejected"))
    })?;
    assert_eq!(delta.unit(), "bytes");
    assert!(matches!(delta.amount(), VizAmount::Exact { value } if value.get() == -5));
    Ok(())
}

#[test]
fn rejects_lossy_or_contradictory_quantities() {
    for value in [
        json!({"unit": "items", "state": "exact", "value": "-1"}),
        json!({"unit": "bytes", "state": "exact", "value": "-0"}),
        json!({"unit": "bytes", "state": "exact", "value": "+1"}),
        json!({"unit": "bytes", "state": "exact", "value": "01"}),
        json!({"unit": "bytes", "state": "exact", "value": 1}),
        json!({"unit": "bytes", "state": "exact", "value": "18446744073709551616"}),
        json!({"unit": "bytes", "state": "exact", "value": "-18446744073709551616"}),
        json!({"unit": "bytes", "state": "lower_bound", "value": "-1"}),
        json!({"unit": "bytes", "state": "unknown", "value": null, "reasonKey": "space.data_insufficient"}),
        json!({"unit": "bytes", "state": "unknown", "reasonKey": ""}),
        json!({"unit": "bytes", "state": "unknown"}),
        json!({"unit": "bytes", "state": "exact", "value": "0", "reasonKey": "space.data_insufficient"}),
    ] {
        assert!(from_value::<VizQuantity>(value.clone()).is_err(), "{value}");
    }
}
