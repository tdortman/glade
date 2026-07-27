use backend_cuda::CudaBackend;
use glade_core::{Backend, BackendResult, rewrite};
use std::{fs, process::Command};

#[path = "support/c_family.rs"]
mod c_family;

#[test]
fn runs_cpp_structural_suite_for_cuda() {
    let backend = CudaBackend;
    c_family::formats_structural_containers_and_markers(&backend);
    c_family::keeps_trailing_comments_with_their_declaration(&backend);
    c_family::preserves_attributes_and_conditional_preprocessor_subtrees(&backend);
    c_family::accepts_cuda_extensions_in_cpp_family(&backend);
    c_family::formats_preprocessor_boundaries(&backend);
}

#[test]
fn formats_cuda_kernel_source_with_kernel_launches() {
    let source = b"__global__ void kernel() {
    kernel<<<1, 1>>>();
    kernel<<<1, 1>>>();
}
int host_helper() { return 0; }
__device__ void helper() {}
";
    let expected = b"__global__ void kernel() {
    kernel<<<1, 1>>>();
    kernel<<<1, 1>>>();
}

int host_helper() { return 0; }
__device__ void helper() {}
";
    let backend = CudaBackend;
    let BackendResult::Ready(plan) = backend.plan(source) else {
        panic!("CUDA fixture was rejected");
    };

    assert_eq!(
        rewrite(source, &plan.boundaries).expect("safe CUDA plan"),
        expected
    );
}

#[test]
fn selects_cuh_files_through_the_cli() {
    let path = std::env::temp_dir().join(format!("glade-cuda-{}.cuh", std::process::id()));
    let source = b"__global__ void kernel() {
}
__device__ void helper() {}
";
    let expected = b"__global__ void kernel() {
}

__device__ void helper() {}
";
    fs::write(&path, source).expect("write CUDA header fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_glade"))
        .arg("format")
        .arg(&path)
        .output()
        .expect("formatter runs");

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(fs::read(&path).expect("read CUDA header fixture"), expected);
    fs::remove_file(path).expect("remove CUDA header fixture");
}
