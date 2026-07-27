use super::ContractFixture;

pub const FIXTURE: ContractFixture = ContractFixture {
    extension: "cu",
    formatting_source: b"__global__ void first() {
    int value;
}
__device__ void second() {}
",
    formatting_expected: b"__global__ void first() {
    int value;
}

__device__ void second() {}
",
    barrier_source: b"__global__ void first() {}

// standalone
__device__ void second() {}
",
    malformed_source: b"__global__ void broken(",
};
