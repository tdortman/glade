use backend_cpp::CppBackend;

#[path = "support/c_family.rs"]
mod c_family;

#[test]
fn formats_cpp_structural_containers_and_markers() {
    let backend = CppBackend;
    c_family::formats_structural_containers_and_markers(&backend);
}

#[test]
fn keeps_cpp_trailing_comments_with_their_declaration() {
    let backend = CppBackend;
    c_family::keeps_trailing_comments_with_their_declaration(&backend);
}

#[test]
fn preserves_cpp_attributes_and_conditional_preprocessor_subtrees() {
    let backend = CppBackend;
    c_family::preserves_attributes_and_conditional_preprocessor_subtrees(&backend);
}

#[test]
fn accepts_cuda_extensions_in_cpp_headers() {
    let backend = CppBackend;
    c_family::accepts_cuda_extensions_in_cpp_family(&backend);
}

#[test]
fn formats_cpp_preprocessor_boundaries() {
    let backend = CppBackend;
    c_family::formats_preprocessor_boundaries(&backend);
}
