pub mod cpp;
pub mod cuda;
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
        "cpp" => Some(cpp::FIXTURE),
        "cuda" => Some(cuda::FIXTURE),
        "rust" => Some(rust::FIXTURE),
        _ => None,
    }
}
