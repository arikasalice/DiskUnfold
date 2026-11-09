use super::{Condition, Error, Predicate, PredicateEvaluator, Record, Registry, Truth};

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

pub(super) enum Plan<'a> {
    Leaf(&'a Predicate, PredicateEvaluator),
    All(Vec<Plan<'a>>),
    Any(Vec<Plan<'a>>),
    Not(Box<Plan<'a>>),
}

/// Resolves strategy revisions once, before any item is evaluated.
pub(super) fn prepare<'a>(
    registry: &Registry,
    condition: &'a Condition,
    depth: usize,
) -> Result<Plan<'a>, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::InvalidCondition);
    }
    match condition {
        Condition::Predicate(predicate) => {
            let (id, revision) = predicate.strategy();
            if matches!(predicate, Predicate::Registered { .. })
                && matches!(id, "pred.name" | "pred.type" | "pred.size")
            {
                return Err(Error::UnknownPredicate);
            }
            let evaluate = registry
                .predicates
                .get(&(id.to_owned(), revision))
                .ok_or(Error::UnknownPredicate)?;
            Ok(Plan::Leaf(predicate, *evaluate))
        }
        Condition::All(children) => Ok(Plan::All(prepare_children(registry, children, depth)?)),
        Condition::Any(children) => Ok(Plan::Any(prepare_children(registry, children, depth)?)),
        Condition::Not(child) => Ok(Plan::Not(Box::new(prepare(registry, child, depth + 1)?))),
    }
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
    pub(super) fn evaluate(&self, item: &Record<'_>) -> Truth {
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
