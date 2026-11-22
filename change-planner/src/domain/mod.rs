//! Pure plan-version decisions.
mod effect;
mod hash;
mod normalize;
mod protection;

pub(crate) use effect::{expected_effect, restore_restriction};
pub(crate) use hash::plan_hash;
pub(crate) use normalize::normalize;
pub(crate) use protection::{
    evaluate_protection, plan_reason, plan_state, platform_unavailable, standard_evaluators,
};
