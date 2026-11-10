use du_core_contracts::SizeValue;

/// Raw component names relative to the root; the root path is empty.
#[derive(Clone, Debug)]
pub struct Observation {
    pub path: Vec<Vec<u8>>,
    pub size: Option<SizeValue>,
}
