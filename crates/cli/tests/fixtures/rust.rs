use super::ContractFixture;

pub const FIXTURE: ContractFixture = ContractFixture {
    extension: "rs",
    formatting_source: b"struct First {
    value: i32,
}
fn second() {}
",
    formatting_expected: b"struct First {
    value: i32,
}

fn second() {}
",
    barrier_source: b"fn first() {}

// standalone
fn second() {}
",
    malformed_source: b"fn broken(",
};
