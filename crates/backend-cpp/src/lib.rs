use backend_c_family::plan_source;
use glade_core::{Backend, BackendResult};

pub struct CppBackend;
const BACKEND_ID: &str = "cpp";

impl Backend for CppBackend {
    fn id(&self) -> &'static str {
        BACKEND_ID
    }

    fn plan(&self, source: &[u8]) -> BackendResult {
        let cuda_language = tree_sitter_cuda::LANGUAGE.into();
        let cpp_language = tree_sitter_cpp::LANGUAGE.into();
        plan_source(source, &[&cpp_language, &cuda_language], BACKEND_ID, "C++")
    }
}
