//! F06 visualization quantity, independent of chart format and geometry.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

/// A decimal string preserving an unsigned count or the sign of a byte delta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub struct VizDecimal(i128);

impl VizDecimal {
    /// Returns the exact signed integer without converting to a floating point value.
    pub fn get(self) -> i128 {
        self.0
    }
}

impl Serialize for VizDecimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for VizDecimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let value = raw.parse::<i128>().map_err(D::Error::custom)?;
        if !in_range(value) || raw != value.to_string() {
            return Err(D::Error::custom("noncanonical visualization quantity"));
        }
        Ok(Self(value))
    }
}

/// Evidence for one quantity. Unknown is distinct from an exact or lower-bound zero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VizAmount {
    Exact {
        value: VizDecimal,
    },
    LowerBound {
        value: VizDecimal,
    },
    Unknown {
        #[serde(rename = "reasonKey")]
        reason_key: String,
    },
}

/// One measured, bounded, or unknown quantity with an open unit key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct VizQuantity {
    unit: String,
    #[serde(flatten)]
    amount: VizAmount,
}

impl VizQuantity {
    /// Constructs an exact quantity. Counts cannot be negative.
    pub fn exact(unit: &str, value: i128) -> Option<Self> {
        Self::new(
            unit.to_owned(),
            VizAmount::Exact {
                value: VizDecimal(value),
            },
        )
    }

    /// Constructs a nonnegative lower bound for the specified unit.
    pub fn lower_bound(unit: &str, value: u64) -> Option<Self> {
        Self::new(
            unit.to_owned(),
            VizAmount::LowerBound {
                value: VizDecimal(i128::from(value)),
            },
        )
    }

    /// Constructs an unknown quantity and retains its reason.
    pub fn unknown(unit: &str, reason_key: String) -> Option<Self> {
        Self::new(unit.to_owned(), VizAmount::Unknown { reason_key })
    }

    /// Returns the registered unit string, including keys introduced by a later strategy.
    pub fn unit(&self) -> &str {
        &self.unit
    }

    /// Returns the reported amount and its evidence state.
    pub fn amount(&self) -> &VizAmount {
        &self.amount
    }

    fn new(unit: String, amount: VizAmount) -> Option<Self> {
        let valid = match &amount {
            VizAmount::Exact { value } => in_range(value.0) && (unit != "items" || value.0 >= 0),
            VizAmount::LowerBound { value } => in_range(value.0) && value.0 >= 0,
            VizAmount::Unknown { reason_key } => !reason_key.trim().is_empty(),
        };
        (valid && !unit.trim().is_empty()).then_some(Self { unit, amount })
    }
}

impl<'de> Deserialize<'de> for VizQuantity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            unit: String,
            #[serde(flatten)]
            amount: VizAmount,
        }
        let input = serde_json::Value::deserialize(deserializer)?;
        let unknown = input.get("state").and_then(serde_json::Value::as_str) == Some("unknown");
        if (unknown && input.get("value").is_some())
            || (!unknown && input.get("reasonKey").is_some())
        {
            return Err(D::Error::custom("quantity fields do not match its state"));
        }
        let wire: Wire = serde_json::from_value(input).map_err(D::Error::custom)?;
        Self::new(wire.unit, wire.amount)
            .ok_or_else(|| D::Error::custom("invalid visualization quantity"))
    }
}

fn in_range(value: i128) -> bool {
    value.unsigned_abs() <= u128::from(u64::MAX)
}
