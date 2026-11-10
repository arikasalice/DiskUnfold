//! GUI query page admission forms shared by search and folder browsing.
use crate::Id;
use serde::Deserialize;

/// A position within a registered, version-pinned query result.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryPagePosition {
    pub query_ref: Id,
    pub result_version: Id,
    pub start: u32,
    pub count: u32,
}

/// A continuation token bound to a GUI session and registered query.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryPageContinuation {
    pub next_cursor: String,
    #[serde(
        default,
        deserialize_with = "provided_count",
        skip_serializing_if = "Option::is_none"
    )]
    pub count: Option<u32>,
}

fn provided_count<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    u32::deserialize(deserializer).map(Some)
}

/// Exactly one of positional admission or authenticated continuation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum QueryPageRequest {
    Position(QueryPagePosition),
    Continuation(QueryPageContinuation),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn admits_only_the_two_published_shapes() {
        let id = "018f0000-0000-7000-8000-000000000001";
        assert!(
            serde_json::from_value::<QueryPageRequest>(
                json!({"queryRef":id,"resultVersion":id,"start":0,"count":1})
            )
            .is_ok()
        );
        assert!(
            serde_json::from_value::<QueryPageRequest>(json!({"nextCursor":"s1.token.mac"}))
                .is_ok()
        );
        assert!(
            serde_json::from_value::<QueryPageRequest>(
                json!({"nextCursor":"s1.token.mac","count":10})
            )
            .is_ok()
        );
        assert!(
            serde_json::from_value::<QueryPageRequest>(
                json!({"queryRef":id,"resultVersion":id,"start":0})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<QueryPageRequest>(json!({"queryRef":id,"resultVersion":id,"start":0,"count":1,"nextCursor":"s1.token.mac"})).is_err());
        assert!(
            serde_json::from_value::<QueryPageRequest>(
                json!({"nextCursor":"s1.token.mac","rowKind":"folder"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<QueryPageRequest>(
                json!({"nextCursor":"s1.token.mac","count":null})
            )
            .is_err()
        );
    }
}
