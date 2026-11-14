use super::{Condition, Error, Predicate, PredicateEvaluator, Record, Registry, Truth};

/// Validates a registered predicate operand without accessing result items.
pub type OperandValidator = fn(&[u8]) -> bool;

const MAX_DEPTH: usize = 64;
const MAX_CHILDREN: usize = 256;

/// Bounds recursion before normalization; normalized groups are checked again by `prepare`.
pub(super) fn check_shape(condition: &Condition, depth: usize) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        return Err(Error::InvalidCondition);
    }
    match condition {
        Condition::Predicate(_) => Ok(()),
        Condition::All(children) | Condition::Any(children) => {
            if children.len() > MAX_CHILDREN {
                return Err(Error::InvalidCondition);
            }
            for child in children {
                check_shape(child, depth + 1)?;
            }
            Ok(())
        }
        Condition::Not(child) => check_shape(child, depth + 1),
    }
}

pub(crate) enum Plan<'a> {
    Leaf(&'a Predicate, PredicateEvaluator),
    All(Vec<Plan<'a>>),
    Any(Vec<Plan<'a>>),
    Not(Box<Plan<'a>>),
}

/// Resolves strategy revisions once, before any item is evaluated.
pub(crate) fn prepare<'a>(
    registry: &Registry,
    condition: &'a Condition,
    depth: usize,
) -> Result<Plan<'a>, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::InvalidCondition);
    }
    match condition {
        Condition::Predicate(predicate) => {
            Ok(Plan::Leaf(predicate, resolve_leaf(registry, predicate)?))
        }
        Condition::All(children) => Ok(Plan::All(prepare_children(registry, children, depth)?)),
        Condition::Any(children) => Ok(Plan::Any(prepare_children(registry, children, depth)?)),
        Condition::Not(child) => Ok(Plan::Not(Box::new(prepare(registry, child, depth + 1)?))),
    }
}

/// Finds the evaluator for one leaf. A registered predicate cannot shadow a built-in ID.
fn resolve_leaf(registry: &Registry, predicate: &Predicate) -> Result<PredicateEvaluator, Error> {
    let (id, revision) = predicate.strategy();
    if matches!(predicate, Predicate::Registered { .. })
        && matches!(id, "pred.name" | "pred.type" | "pred.size")
    {
        return Err(Error::UnknownPredicate);
    }
    registry
        .predicates
        .get(&(id.to_owned(), revision))
        .copied()
        .ok_or(Error::UnknownPredicate)
}

/// The decision of one condition node in a diagnostic walk of a single record.
pub(crate) struct NodeTruth {
    /// Child indices from the condition root; the root's path is empty.
    pub(crate) path: Vec<u32>,
    /// Strategy ID and revision, present only for a leaf.
    pub(crate) predicate: Option<(String, u32)>,
    /// `None` when the node depends on a predicate the registry does not hold.
    pub(crate) truth: Option<Truth>,
}

/// Evaluates every node of `condition` for one record, in pre-order (a node precedes its
/// children, children follow their order in the condition). Unlike `Plan::evaluate` it does
/// not stop at a decisive child, so each node has a decision; the combined decision of a node
/// is the same F04 three-valued result. A node whose subtree holds an unregistered predicate
/// has no decision, because the rule as a whole cannot be evaluated.
pub(crate) fn explain(
    registry: &Registry,
    condition: &Condition,
    record: &Record<'_>,
) -> Result<Vec<NodeTruth>, Error> {
    let mut nodes = Vec::new();
    explain_node(registry, condition, record, &mut Vec::new(), &mut nodes, 0)?;
    Ok(nodes)
}

fn explain_node(
    registry: &Registry,
    condition: &Condition,
    record: &Record<'_>,
    path: &mut Vec<u32>,
    nodes: &mut Vec<NodeTruth>,
    depth: usize,
) -> Result<Option<Truth>, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::InvalidCondition);
    }
    let slot = nodes.len();
    nodes.push(NodeTruth {
        path: path.clone(),
        predicate: None,
        truth: None,
    });
    let truth = match condition {
        Condition::Predicate(predicate) => {
            let (id, revision) = predicate.strategy();
            if let Some(node) = nodes.get_mut(slot) {
                node.predicate = Some((id.to_owned(), revision));
            }
            match resolve_leaf(registry, predicate) {
                Ok(evaluate) => Some(evaluate(record, predicate)),
                Err(Error::UnknownPredicate) => None,
                Err(error) => return Err(error),
            }
        }
        Condition::All(children) | Condition::Any(children) => {
            let all = matches!(condition, Condition::All(_));
            let mut truths = Vec::with_capacity(children.len());
            for (index, child) in children.iter().enumerate() {
                path.push(u32::try_from(index).map_err(|_| Error::InvalidCondition)?);
                truths.push(explain_node(
                    registry,
                    child,
                    record,
                    path,
                    nodes,
                    depth + 1,
                )?);
                path.pop();
            }
            fold(all, &truths)
        }
        Condition::Not(child) => {
            path.push(0);
            let inner = explain_node(registry, child, record, path, nodes, depth + 1)?;
            path.pop();
            inner.map(|truth| match truth {
                Truth::Match => Truth::NoMatch,
                Truth::NoMatch => Truth::Match,
                Truth::Unknown => Truth::Unknown,
            })
        }
    };
    if let Some(node) = nodes.get_mut(slot) {
        node.truth = truth;
    }
    Ok(truth)
}

/// Combines already evaluated children like `combine`: a decisive child wins, otherwise
/// any unknown child makes the group unknown. A missing child decision leaves the group
/// without a decision.
fn fold(all: bool, truths: &[Option<Truth>]) -> Option<Truth> {
    let mut unknown = false;
    let mut decisive = false;
    for truth in truths {
        match (*truth)? {
            Truth::NoMatch if all => decisive = true,
            Truth::Match if !all => decisive = true,
            Truth::Unknown => unknown = true,
            _ => {}
        }
    }
    Some(if decisive {
        if all { Truth::NoMatch } else { Truth::Match }
    } else if unknown {
        Truth::Unknown
    } else if all {
        Truth::Match
    } else {
        Truth::NoMatch
    })
}

/// Checks registered operands through the exact selected revision's validator.
pub(super) fn validate_operands(registry: &Registry, condition: &Condition) -> Result<(), Error> {
    match condition {
        Condition::Predicate(Predicate::Registered {
            id,
            revision,
            operand,
        }) => {
            let validator = registry
                .operand_validators
                .get(&(id.clone(), *revision))
                .ok_or(Error::InvalidCondition)?;
            if !validator(operand) {
                return Err(Error::InvalidCondition);
            }
        }
        Condition::All(children) | Condition::Any(children) => {
            for child in children {
                validate_operands(registry, child)?;
            }
        }
        Condition::Not(child) => validate_operands(registry, child)?,
        Condition::Predicate(_) => {}
    }
    Ok(())
}

fn prepare_children<'a>(
    registry: &Registry,
    children: &'a [Condition],
    depth: usize,
) -> Result<Vec<Plan<'a>>, Error> {
    if children.len() > MAX_CHILDREN {
        return Err(Error::InvalidCondition);
    }
    children
        .iter()
        .map(|child| prepare(registry, child, depth + 1))
        .collect()
}

impl Plan<'_> {
    pub(crate) fn evaluate(&self, item: &Record<'_>) -> Truth {
        match self {
            Self::Leaf(predicate, evaluate) => evaluate(item, predicate),
            Self::Not(child) => match child.evaluate(item) {
                Truth::Match => Truth::NoMatch,
                Truth::NoMatch => Truth::Match,
                Truth::Unknown => Truth::Unknown,
            },
            Self::All(children) => combine(children, item, true),
            Self::Any(children) => combine(children, item, false),
        }
    }
}

fn combine(children: &[Plan<'_>], item: &Record<'_>, all: bool) -> Truth {
    let mut unknown = false;
    for child in children {
        match child.evaluate(item) {
            Truth::NoMatch if all => return Truth::NoMatch,
            Truth::Match if !all => return Truth::Match,
            Truth::Unknown => unknown = true,
            _ => {}
        }
    }
    if unknown {
        Truth::Unknown
    } else if all {
        Truth::Match
    } else {
        Truth::NoMatch
    }
}

impl Registry {
    /// Registers a predicate with its operand validator for pre-admission preparation.
    pub fn register_checked_predicate(
        &mut self,
        id: &str,
        revision: u32,
        evaluator: PredicateEvaluator,
        validator: OperandValidator,
    ) -> Result<(), Error> {
        self.register_predicate(id, revision, evaluator)?;
        self.operand_validators
            .insert((id.to_owned(), revision), validator);
        Ok(())
    }

    /// Validates a normalized query before promotion without visiting result items.
    /// Registered predicates without a validator fail closed. This checks the supplied
    /// model only; syntax conversion, channel policy and source resolution belong to callers.
    pub fn prepare_search(
        &self,
        condition: &Condition,
        sorts: &[super::Sort<'_>],
    ) -> Result<(), Error> {
        check_shape(condition, 0)?;
        let normalized = condition.clone().normalize();
        check_shape(&normalized, 0)?;
        prepare(self, &normalized, 0)?;
        validate_operands(self, &normalized)?;
        if sorts.is_empty() {
            return Err(Error::UnknownSort);
        }
        self.validate_sorts(sorts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IngestSink;
    use du_core_contracts::Id;
    use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};

    fn matches(_: &Record<'_>, _: &Predicate) -> Truth {
        Truth::Match
    }
    fn rejects(_: &Record<'_>, _: &Predicate) -> Truth {
        Truth::NoMatch
    }
    fn unknown(_: &Record<'_>, _: &Predicate) -> Truth {
        Truth::Unknown
    }
    fn leaf(id: &str) -> Condition {
        Condition::Predicate(Predicate::Registered {
            id: id.into(),
            revision: 1,
            operand: vec![],
        })
    }
    fn three_valued_registry() -> Result<Registry, String> {
        let mut registry = Registry::default();
        for (id, evaluate) in [
            ("pred.m", matches as PredicateEvaluator),
            ("pred.n", rejects),
            ("pred.u", unknown),
        ] {
            registry
                .register_predicate(id, 1, evaluate)
                .map_err(|e| format!("{e:?}"))?;
        }
        Ok(registry)
    }
    fn root_version() -> Result<std::sync::Arc<crate::ResultVersion>, String> {
        let id = |n| Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"));
        let sink = IngestSink::default();
        let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
        build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        build
            .seal(id(2)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))
    }
    fn shapes(depth: usize) -> Vec<Condition> {
        let mut all = vec![
            leaf("pred.m"),
            leaf("pred.n"),
            leaf("pred.u"),
            Condition::All(vec![]),
            Condition::Any(vec![]),
        ];
        for _ in 0..depth {
            let previous = all.clone();
            for a in &previous {
                all.push(Condition::Not(Box::new(a.clone())));
                for b in &previous {
                    all.push(Condition::All(vec![a.clone(), b.clone()]));
                    all.push(Condition::Any(vec![a.clone(), b.clone()]));
                }
            }
        }
        all
    }

    #[test]
    fn explanation_root_equals_plan_evaluation_for_every_shape() -> Result<(), String> {
        let registry = three_valued_registry()?;
        let version = root_version()?;
        let item = version.items().first().ok_or("missing root")?;
        let record = Record {
            item,
            path: &[],
            version: &version,
        };
        for condition in shapes(2) {
            let plan = prepare(&registry, &condition, 0).map_err(|e| format!("{e:?}"))?;
            let nodes = explain(&registry, &condition, &record).map_err(|e| format!("{e:?}"))?;
            let root = nodes.first().ok_or("missing root node")?;
            assert_eq!(root.truth, Some(plan.evaluate(&record)), "{condition:?}");
        }
        Ok(())
    }

    #[test]
    fn explanation_is_pre_order_and_unregistered_subtrees_have_no_decision() -> Result<(), String> {
        let registry = three_valued_registry()?;
        let version = root_version()?;
        let item = version.items().first().ok_or("missing root")?;
        let record = Record {
            item,
            path: &[],
            version: &version,
        };
        let condition = Condition::All(vec![
            Condition::Not(Box::new(leaf("pred.m"))),
            Condition::Any(vec![leaf("pred.absent"), leaf("pred.u")]),
        ]);
        let nodes = explain(&registry, &condition, &record).map_err(|e| format!("{e:?}"))?;
        let observed: Vec<_> = nodes
            .iter()
            .map(|node| (node.path.clone(), node.truth))
            .collect();
        assert_eq!(
            observed,
            [
                (vec![], None),
                (vec![0], Some(Truth::NoMatch)),
                (vec![0, 0], Some(Truth::Match)),
                (vec![1], None),
                (vec![1, 0], None),
                (vec![1, 1], Some(Truth::Unknown)),
            ]
        );
        assert_eq!(nodes[4].predicate, Some(("pred.absent".to_owned(), 1)));
        assert_eq!(nodes[1].predicate, None);
        Ok(())
    }

    #[test]
    fn prepared_plan_keeps_resolved_function_without_registry_reads() -> Result<(), String> {
        let mut registry = Registry::default();
        registry
            .register_predicate("pred.test", 1, matches)
            .map_err(|e| format!("{e:?}"))?;
        let condition = Condition::Predicate(Predicate::Registered {
            id: "pred.test".into(),
            revision: 1,
            operand: vec![],
        });
        let plan = prepare(&registry, &condition, 0).map_err(|e| format!("{e:?}"))?;
        registry.predicates.insert(("pred.test".into(), 1), rejects);
        let id = |n| Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"));
        let sink = IngestSink::default();
        let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
        build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        let version = build
            .seal(id(2)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let item = version.items().first().ok_or("missing root")?;
        let record = Record {
            item,
            path: &[],
            version: &version,
        };
        assert_eq!(plan.evaluate(&record), Truth::Match);
        Ok(())
    }
}
