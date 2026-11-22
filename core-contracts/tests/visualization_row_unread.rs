use du_core_contracts::visualization::rows::VizRow;
use serde_json::{Value, json};
const ID: &str = "018f0000-0000-7000-8000-000000000001";
fn row() -> Value {
    json!({"itemRef":{"resultVersion":ID,"itemNo":1},"groupRef":null,"displayName":"a","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"20"},"ratio":"-0.25","denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"80"}},"legendClassKey":"future.class","displayFlags":["future.flag"]})
}
/// The same row as a group row, which is the only kind that may carry a label key.
fn group_row() -> Value {
    let mut group = row();
    group["itemRef"] = Value::Null;
    group["groupRef"] = json!("g1");
    group
}
#[test]
fn a_folder_row_says_how_it_could_not_be_read() -> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::visualization::rows::VizUnread;
    let with = |unread: Value| {
        let mut value = row();
        value["unread"] = unread;
        value
    };
    for (wire, expected) in [
        (json!({"kind":"itself"}), VizUnread::Itself),
        (
            json!({"kind":"inside","count":"3"}),
            VizUnread::Inside {
                count: Some("3".into()),
            },
        ),
        (json!({"kind":"inside"}), VizUnread::Inside { count: None }),
        // An unknown kind is read, so a newer sender does not break an older receiver.
        (json!({"kind":"later"}), VizUnread::Unknown),
    ] {
        let parsed: VizRow = serde_json::from_value(with(wire.clone()))?;
        assert_eq!(parsed.unread, Some(expected), "{wire}");
    }
    assert_eq!(VizUnread::inside(Some(0)), None);
    for bad in [
        Value::Null,
        json!({"kind":"inside","count":"0"}),
        json!({"kind":"inside","count":"03"}),
        json!({"kind":"inside","count":"-3"}),
        json!({"kind":"inside","count":null}),
    ] {
        assert!(
            serde_json::from_value::<VizRow>(with(bad.clone())).is_err(),
            "{bad}"
        );
    }
    let mut group = group_row();
    group["unread"] = json!({"kind":"itself"});
    assert!(serde_json::from_value::<VizRow>(group).is_err());
    Ok(())
}
