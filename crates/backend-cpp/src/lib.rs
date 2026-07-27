use backend_c_family::plan_source;
use glade_core::{Backend, BackendResult};

pub struct CppBackend;

const BACKEND_ID: &str = "cpp";

impl Backend for CppBackend {
    fn id(&self) -> &'static str {
        BACKEND_ID
    }

    fn plan(&self, source: &[u8]) -> BackendResult {
        let language = tree_sitter_cpp::LANGUAGE.into();
        plan_source(source, &language, BACKEND_ID, "C++")
    }
}
