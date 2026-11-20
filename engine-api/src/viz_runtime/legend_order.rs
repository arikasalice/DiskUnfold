//! The order of a model's legend and the classes it leaves out.
use std::collections::BTreeMap;

/// The classes of `found` that a legend lists, in `order`. A class that the order
/// does not name (a folder) is left out. Without an order every class is listed,
/// ordered by key.
pub(super) fn listed<T>(mut found: BTreeMap<String, T>, order: &[String]) -> Vec<(String, T)> {
    if order.is_empty() {
        return found.into_iter().collect();
    }
    order
        .iter()
        .filter_map(|key| found.remove_entry(key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::listed;
    use std::collections::BTreeMap;

    #[test]
    fn a_legend_follows_its_order_and_leaves_out_unnamed_classes() {
        let found: BTreeMap<_, _> = [("type.video", 1), ("type.folder", 2), ("type.audio", 3)]
            .map(|(key, value)| (key.to_owned(), value))
            .into();
        let order = ["type.audio", "type.image", "type.video"].map(str::to_owned);
        assert_eq!(
            listed(found.clone(), &order),
            [("type.audio".to_owned(), 3), ("type.video".to_owned(), 1)]
        );
        assert_eq!(listed(found, &[]).len(), 3);
    }
}
