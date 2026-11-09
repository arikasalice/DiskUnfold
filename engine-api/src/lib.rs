//! Own the shared entry point and use case registration.

pub mod api;
mod contracts;
pub mod error;
mod pipeline;
mod registry;

#[cfg(test)]
mod tests;
