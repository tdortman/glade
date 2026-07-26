pub mod rust;

#[derive(Clone, Copy)]
pub struct ContractFixture {
    pub extension: &'static str,
    pub formatting_source: &'static [u8],
    pub formatting_expected: &'static [u8],
    pub barrier_source: &'static [u8],
    pub malformed_source: &'static [u8],
}

#[must_use]
pub fn for_backend(id: &str) -> Option<ContractFixture> {
    match id {
        "rust" => Some(rust::FIXTURE),
        _ => None,
    }
}
