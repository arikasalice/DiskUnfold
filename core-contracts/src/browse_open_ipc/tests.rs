use super::*;
use serde_json::{Value, json};

#[test]
fn open_uses_published_shapes_and_rejects_mixed_conditions()
-> Result<(), Box<dyn std::error::Error>> {
    let id = "018f0000-0000-7000-8000-000000000001";
    let input = json!({"viewKind":"tree_branch","requestSeq":7,"sourceRef":{"kind":"scan","ref":id},"resultVersion":id,"condition":{"kind":"all","children":[{"kind":"predicate","predicate":{"kind":"size","basis":"logical","op":"at_least","bytes":"1024"}},{"kind":"not","child":{"kind":"predicate","predicate":{"kind":"name","op":"equal","encoding":"windows_utf16le","value":"YQA"}}}]},"sortKeys":[{"id":"sort.path","revision":1,"descending":false}],"pageSize":200});
    let parsed: BrowseOpenRequest = serde_json::from_value(input.clone())?;
    assert_eq!(serde_json::to_value(parsed)?, input);
    for value in [
        json!({"viewKind":"list"}),
        json!({"viewKind":"list","requestSeq":1,"correlationId":id}),
        json!({"viewKind":"list","requestSeq":1,"condition":{"kind":"all","children":[],"child":{"kind":"all","children":[]}}}),
        json!({"viewKind":"list","requestSeq":1,"condition":{"kind":"predicate","predicate":{"kind":"size","basis":"logical","op":"at_least","bytes":1024}}}),
        json!({"viewKind":"list","requestSeq":1,"sourceRef":{"kind":"future","ref":id}}),
    ] {
        assert!(serde_json::from_value::<BrowseOpenRequest>(value).is_err());
    }
    let minimal = serde_json::to_value(serde_json::from_value::<BrowseOpenRequest>(
        json!({"viewKind":"list","requestSeq":1}),
    )?)?;
    assert_eq!(minimal, json!({"viewKind":"list","requestSeq":1}));
    let encoded: Value = serde_json::to_value(QueryCondition::All { children: vec![] })?;
    assert_eq!(encoded, json!({"kind":"all","children":[]}));
    Ok(())
}
