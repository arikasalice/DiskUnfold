use crate::{Id, ItemRef};
use serde::{Deserialize, de::value::MapAccessDeserializer};
use std::marker::PhantomData;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItemReferenceFields {
    result_version: Id,
    item_no: u32,
}

/// A request-local item reference whose object shape rejects unknown fields.
pub(super) struct F03RequestItemRef(ItemRef);

impl From<F03RequestItemRef> for ItemRef {
    fn from(item: F03RequestItemRef) -> Self {
        item.0
    }
}

impl<'de> Deserialize<'de> for F03RequestItemRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields: ItemReferenceFields = deserialize_f03_request_object(deserializer)?;
        Ok(Self(ItemRef {
            result_version: fields.result_version,
            item_no: fields.item_no,
        }))
    }
}

/// Deserializes an F03 request object without accepting positional struct arrays.
pub(crate) fn deserialize_f03_request_object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct ObjectVisitor<T>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for ObjectVisitor<T> {
        type Value = T;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an object")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_map(ObjectVisitor(PhantomData))
}

/// Deserializes the strict F03 total-query request body.
pub(super) fn deserialize_space_total_request<'de, D>(
    deserializer: D,
) -> Result<(Option<Id>, super::SpaceTarget, u32), D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Fields {
        result_version: Option<Id>,
        target: super::SpaceTarget,
        request_seq: u32,
    }

    let fields: Fields = deserialize_f03_request_object(deserializer)?;
    Ok((fields.result_version, fields.target, fields.request_seq))
}

/// Deserializes an item reference inside an F03 command without changing the shared reference type.
pub(crate) fn deserialize_f03_request_item_ref<'de, D>(deserializer: D) -> Result<ItemRef, D::Error>
where
    D: serde::Deserializer<'de>,
{
    F03RequestItemRef::deserialize(deserializer).map(Into::into)
}

/// Deserializes an optional F03 command item reference, preserving omitted and null values.
pub(crate) fn deserialize_optional_f03_request_item_ref<'de, D>(
    deserializer: D,
) -> Result<Option<ItemRef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<F03RequestItemRef>::deserialize(deserializer).map(|item| item.map(Into::into))
}
