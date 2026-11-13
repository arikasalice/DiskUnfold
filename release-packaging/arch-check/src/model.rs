use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub(crate) struct Diagnostic {
    pub(crate) rule: String,
    pub(crate) location: String,
    pub(crate) message: String,
}
#[derive(Default, Serialize)]
pub(crate) struct Metrics {
    pub(crate) crate_direct_dependencies: BTreeMap<String, usize>,
    pub(crate) module_edges: usize,
    pub(crate) module_count: usize,
    pub(crate) rust_files: usize,
    pub(crate) rust_lines: usize,
    pub(crate) file_lines: BTreeMap<String, usize>,
    pub(crate) function_complexity: BTreeMap<String, usize>,
    pub(crate) constructor_dependencies: BTreeMap<String, usize>,
    pub(crate) functions: usize,
    pub(crate) max_complexity: usize,
    pub(crate) max_constructor_dependencies: usize,
}
#[derive(Default, Serialize)]
pub(crate) struct Report {
    pub(crate) schema_version: u8,
    pub(crate) metrics: Metrics,
    pub(crate) diagnostics: Vec<Diagnostic>,
}
impl Report {
    pub(crate) fn add(
        &mut self,
        rule: &str,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            rule: rule.into(),
            location: location.into(),
            message: message.into(),
        });
    }
}
