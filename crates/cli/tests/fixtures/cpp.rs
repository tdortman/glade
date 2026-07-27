use super::ContractFixture;

pub const FIXTURE: ContractFixture = ContractFixture {
    extension: "cpp",
    formatting_source: b"class First {
    int value;
};
void second() {}
",
    formatting_expected: b"class First {
    int value;
};

void second() {}
",
    barrier_source: b"void first() {}

// standalone
void second() {}
",
    malformed_source: b"void broken(",
};
