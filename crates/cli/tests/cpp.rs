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
