use super::*;

fn rule<'a>(
    id: &'a str,
    role: Role,
    conditions: &'a [Condition<'a>],
    covers: &'a [&'a str],
) -> Rule<'a> {
    Rule {
        id,
        priority: 1,
        role,
        may_confirm: true,
        conditions,
        covers,
    }
}
fn facts() -> Observation<'static> {
    Observation {
        kind: ResultItemKind::Directory,
        marker_children: Some(&[b"marker"]),
    }
}

#[test]
fn observed_kind_and_marker_support_a_confirmed_role() -> Result<(), InvalidRules> {
    let conditions = [
        Condition::Kind(ResultItemKind::Directory),
        Condition::MarkerChild(b"marker"),
    ];
    let rules = [rule("r1", Role::Cache, &conditions, &[])];
    let result = RuleSet::new(&rules)?.evaluate(&facts());
    assert_eq!(result.verdict, Verdict::Confirmed);
    assert_eq!(result.selected, Some("r1"));
    assert_eq!(
        result.candidates[0].evidence,
        [Evidence::Satisfied, Evidence::Satisfied]
    );
    assert_eq!(result, RuleSet::new(&rules)?.evaluate(&facts()));
    Ok(())
}

#[test]
fn missing_evidence_never_establishes_a_confirmed_role() -> Result<(), InvalidRules> {
    let marker = [Condition::MarkerChild(b"marker")];
    let rules = [rule("r1", Role::Cache, &marker, &[])];
    let unavailable = Observation {
        marker_children: None,
        ..facts()
    };
    assert_eq!(
        RuleSet::new(&rules)?.evaluate(&unavailable).verdict,
        Verdict::Estimated
    );
    let location = [Condition::LocationUnavailable];
    let rules = [rule("r1", Role::Cache, &location, &[])];
    let result = RuleSet::new(&rules)?.evaluate(&facts());
    assert_eq!(result.verdict, Verdict::Unavailable);
    assert!(result.location_unverified);
    assert_eq!(result.selected, None);
    Ok(())
}

#[test]
fn conflicting_roles_are_preserved_and_only_explicit_cover_resolves_them()
-> Result<(), InvalidRules> {
    let condition = [Condition::Kind(ResultItemKind::Directory)];
    let conflict = [
        rule("a", Role::Cache, &condition, &[]),
        rule("b", Role::UserData, &condition, &[]),
    ];
    let result = RuleSet::new(&conflict)?.evaluate(&facts());
    assert_eq!(result.verdict, Verdict::Conflict);
    assert_eq!(result.selected, None);
    assert_eq!(result.candidates.len(), 2);
    let covered = [
        rule("a", Role::Cache, &condition, &["b"]),
        rule("b", Role::UserData, &condition, &[]),
    ];
    assert_eq!(
        RuleSet::new(&covered)?.evaluate(&facts()).selected,
        Some("a")
    );
    Ok(())
}

#[test]
fn contradictory_evidence_and_invalid_rules_never_produce_a_role() -> Result<(), InvalidRules> {
    let file = [Condition::Kind(ResultItemKind::File)];
    let rules = [rule("r1", Role::Cache, &file, &[])];
    assert_eq!(
        RuleSet::new(&rules)?.evaluate(&facts()).verdict,
        Verdict::Unknown
    );
    let duplicate = [
        rule("r1", Role::Cache, &file, &[]),
        rule("r1", Role::UserData, &file, &[]),
    ];
    assert!(RuleSet::new(&duplicate).is_err());
    let cyclic = [
        rule("a", Role::Cache, &file, &["b"]),
        rule("b", Role::UserData, &file, &["a"]),
    ];
    assert!(RuleSet::new(&cyclic).is_err());
    Ok(())
}

#[test]
fn unavailable_covering_rule_does_not_erase_a_conflicting_candidate() -> Result<(), InvalidRules> {
    let uncertain = [Condition::MarkerChild(b"marker")];
    let observed = [Condition::Kind(ResultItemKind::Directory)];
    let rules = [
        rule("a", Role::Cache, &uncertain, &["b"]),
        rule("b", Role::UserData, &observed, &[]),
    ];
    let facts = Observation {
        marker_children: None,
        ..facts()
    };
    let result = RuleSet::new(&rules)?.evaluate(&facts);
    assert_eq!(result.verdict, Verdict::Conflict);
    assert_eq!(result.selected, None);
    Ok(())
}

#[test]
fn unavailable_bundle_is_not_treated_as_no_match() -> Result<(), InvalidRules> {
    let result = evaluate_bundle(None, &facts());
    assert_eq!(result.verdict, Verdict::Unavailable);
    assert_eq!(result.selected, None);
    Ok(())
}

#[test]
fn valid_ruleset_can_be_reused_for_empty_and_contradictory_observations() -> Result<(), InvalidRules>
{
    let condition = [Condition::MarkerChild(b"marker")];
    let rules = [rule("a", Role::Cache, &condition, &[])];
    let index = RuleSet::new(&rules)?;
    assert_eq!(index.evaluate(&facts()).verdict, Verdict::Confirmed);
    let empty = Observation {
        kind: ResultItemKind::Unknown,
        marker_children: Some(&[]),
    };
    assert_eq!(index.evaluate(&empty).verdict, Verdict::Unknown);
    let missing = Observation {
        kind: ResultItemKind::Unknown,
        marker_children: None,
    };
    assert_eq!(index.evaluate(&missing).verdict, Verdict::Estimated);
    assert_eq!(index.evaluate(&facts()).verdict, Verdict::Confirmed);
    Ok(())
}

#[test]
fn chain_with_competing_roles_remains_unresolved() -> Result<(), InvalidRules> {
    let condition = [Condition::Kind(ResultItemKind::Directory)];
    let rules = [
        rule("a", Role::Cache, &condition, &["b"]),
        rule("b", Role::Temp, &condition, &["c"]),
        rule("c", Role::UserData, &condition, &[]),
    ];
    let result = RuleSet::new(&rules)?.evaluate(&facts());
    assert_eq!(result.verdict, Verdict::Conflict);
    assert_eq!(result.selected, None);
    Ok(())
}

#[test]
fn location_unavailable_precedes_conflict_only_without_an_observed_match()
-> Result<(), InvalidRules> {
    let location = [Condition::LocationUnavailable];
    let known = [Condition::Kind(ResultItemKind::Directory)];
    let uncertain = [
        rule("a", Role::Cache, &location, &[]),
        rule("b", Role::UserData, &location, &[]),
    ];
    assert_eq!(
        RuleSet::new(&uncertain)?.evaluate(&facts()).verdict,
        Verdict::Unavailable
    );
    let mixed = [
        rule("a", Role::Cache, &location, &[]),
        rule("b", Role::UserData, &known, &[]),
    ];
    assert_eq!(
        RuleSet::new(&mixed)?.evaluate(&facts()).verdict,
        Verdict::Conflict
    );
    Ok(())
}

#[test]
fn covering_chain_only_surviving_rules_can_remove_explicit_targets() -> Result<(), InvalidRules> {
    let observed = [Condition::Kind(ResultItemKind::Directory)];
    let rules = [
        rule("a", Role::Cache, &observed, &["b"]),
        rule("b", Role::Temp, &observed, &["c"]),
        rule("c", Role::UserData, &observed, &[]),
    ];
    let decision = RuleSet::new(&rules)?.evaluate(&facts());
    assert_eq!(decision.verdict, Verdict::Conflict);
    assert_eq!(decision.candidates.len(), 3);
    let direct = [
        rule("a", Role::Cache, &observed, &["b", "c"]),
        rule("b", Role::Temp, &observed, &["c"]),
        rule("c", Role::UserData, &observed, &[]),
    ];
    let result = RuleSet::new(&direct)?.evaluate(&facts());
    assert_eq!(
        (result.verdict, result.selected),
        (Verdict::Confirmed, Some("a"))
    );
    assert_eq!(result.candidates.len(), 3);
    Ok(())
}

#[test]
fn unavailable_cover_does_not_remove_target_and_order_is_stable() -> Result<(), InvalidRules> {
    let unknown = [Condition::LocationUnavailable];
    let observed = [Condition::Kind(ResultItemKind::Directory)];
    let first = [
        rule("a", Role::Cache, &unknown, &["b"]),
        rule("b", Role::Temp, &observed, &["c"]),
        rule("c", Role::UserData, &observed, &[]),
    ];
    let reversed = [
        rule("c", Role::UserData, &observed, &[]),
        rule("b", Role::Temp, &observed, &["c"]),
        rule("a", Role::Cache, &unknown, &["b"]),
    ];
    let result = RuleSet::new(&first)?.evaluate(&facts());
    assert_eq!(result.verdict, Verdict::Conflict);
    assert_eq!(result, RuleSet::new(&reversed)?.evaluate(&facts()));
    assert_eq!(result.candidates[0].evidence, [Evidence::Unavailable]);
    Ok(())
}

#[test]
fn equal_role_and_direct_cover_resolve_without_spurious_conflict() -> Result<(), InvalidRules> {
    let observed = [Condition::Kind(ResultItemKind::Directory)];
    let rules = [
        rule("a", Role::Cache, &observed, &["b", "c"]),
        rule("b", Role::Cache, &observed, &["c"]),
        rule("c", Role::UserData, &observed, &[]),
    ];
    let result = RuleSet::new(&rules)?.evaluate(&facts());
    assert_eq!(
        (result.verdict, result.selected),
        (Verdict::Confirmed, Some("a"))
    );
    Ok(())
}

#[test]
fn coverage_topology_precedes_priority_and_preserves_equal_survivor_roles()
-> Result<(), InvalidRules> {
    let observed = [Condition::Kind(ResultItemKind::Directory)];
    let mut a = rule("a", Role::Cache, &observed, &["b"]);
    let mut b = rule("b", Role::Temp, &observed, &["c"]);
    let c = rule("c", Role::Cache, &observed, &[]);
    a.priority = -10;
    b.priority = 100;
    let rules = [b, c, a];
    let result = RuleSet::new(&rules)?.evaluate(&facts());
    assert_eq!(
        (result.verdict, result.selected),
        (Verdict::Confirmed, Some("c"))
    );
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        ["b", "c", "a"]
    );
    Ok(())
}
