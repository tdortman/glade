use glade_core::{Backend, BackendResult, rewrite};

pub fn formats_structural_containers_and_markers(backend: &dyn Backend) {
    let source = b"#define VALUE 1
template <typename T>
struct Box {
public:
    T value;
    void reset() {
        value = T{};
        value = T{};
    }
};
extern \"C\" {
void first();
void second();
}
enum Kind {
    one,
    two
};
void labels() {
label:
    first();
    second();
switch (VALUE) {
case 1:
    first();
    second();
    break;
default:
    second();
}
}
";

    let expected = b"#define VALUE 1

template <typename T>
struct Box {
public:
    T value;

    void reset() {
        value = T{};
        value = T{};
    }
};

extern \"C\" {
void first();
void second();
}

enum Kind {
    one,
    two
};

void labels() {
label:
    first();

    second();

switch (VALUE) {
case 1:
    first();

    second();
    break;

default:
    second();
}
}
";

    let BackendResult::Ready(plan) = backend.plan(source) else {
        panic!("{} fixture was rejected", backend.id());
    };

    assert_eq!(
        rewrite(source, &plan.boundaries).expect("safe C-family plan"),
        expected
    );
}

pub fn keeps_trailing_comments_with_their_declaration(backend: &dyn Backend) {
    let source = b"int first; // tail
int second;
";

    let BackendResult::Ready(plan) = backend.plan(source) else {
        panic!("{} comment fixture was rejected", backend.id());
    };

    assert_eq!(
        rewrite(source, &plan.boundaries).expect("safe C-family plan"),
        source
    );
}

pub fn preserves_attributes_and_conditional_preprocessor_subtrees(backend: &dyn Backend) {
    let source = b"namespace outer {
[[nodiscard]]
int first();
#if FLAG
int conditional;
#endif
int second();
}
";

    let expected = b"namespace outer {
[[nodiscard]]
int first();

#if FLAG
int conditional;
#endif

int second();
}
";

    let BackendResult::Ready(plan) = backend.plan(source) else {
        panic!("{} namespace fixture was rejected", backend.id());
    };

    assert_eq!(
        rewrite(source, &plan.boundaries).expect("safe C-family plan"),
        expected
    );
}

pub fn accepts_cuda_extensions_in_cpp_family(backend: &dyn Backend) {
    let source = b"template <typename T>
__global__ void kernel(T* output) {
    output[0] = T{};
}

void launch(char* output) {
    kernel<<<1, 1>>>(output);
}
";

    let BackendResult::Ready(plan) = backend.plan(source) else {
        panic!("{} CUDA extension fixture was rejected", backend.id());
    };

    assert_eq!(
        rewrite(source, &plan.boundaries).expect("safe CUDA extension plan"),
        source
    );
}

pub fn formats_preprocessor_boundaries(backend: &dyn Backend) {
    let source = b"#pragma once
#include <vector>
#include <string>
namespace example {}
";

    let expected = b"#pragma once

#include <vector>
#include <string>

namespace example {}
";

    let BackendResult::Ready(plan) = backend.plan(source) else {
        panic!("{} preprocessor fixture was rejected", backend.id());
    };

    assert_eq!(
        rewrite(source, &plan.boundaries).expect("safe preprocessor plan"),
        expected
    );
}
