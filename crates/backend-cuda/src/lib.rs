use backend_c_family::plan_source;
use glade_core::{Backend, BackendResult};
pub struct CudaBackend;
const BACKEND_ID: &str = "cuda";

impl Backend for CudaBackend {
    fn id(&self) -> &'static str {
        BACKEND_ID
    }

    fn plan(&self, source: &[u8]) -> BackendResult {
        let language = tree_sitter_cuda::LANGUAGE.into();
        plan_source(source, &[&language], BACKEND_ID, "CUDA")
    }
}
