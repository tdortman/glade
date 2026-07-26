use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

mod fixtures;

use glade_cli::{
    BackendRegistration, RegistryError, backend_for_extension, backend_registry, validate_registry,
};

use glade_core::{Backend, BackendResult, FormatPlan, rewrite};

fn registered_backends() -> impl Iterator<Item = &'static dyn Backend> {
    backend_registry()
        .iter()
        .map(|registration| registration.backend)
}

fn fixture_for(backend: &dyn Backend) -> fixtures::ContractFixture {
    fixtures::for_backend(backend.id())
        .unwrap_or_else(|| panic!("missing contract fixture for {}", backend.id()))
}

#[test]
fn registered_extensions_are_unique_and_select_the_declared_backend() {
    validate_registry(backend_registry()).expect("compiled-in backend registry is valid");

    for registration in backend_registry() {
        for &extension in registration.extensions {
            let backend = backend_for_extension(extension).expect("registered extension resolves");
            assert_eq!(backend.id(), registration.backend.id());
        }
    }
}

#[test]
fn every_registered_backend_preserves_selected_source_ranges() {
    for backend in registered_backends() {
        let fixture = fixture_for(backend);

        let BackendResult::Ready(plan) = backend.plan(fixture.formatting_source) else {
            panic!("{} rejected its formatting fixture", backend.id());
        };

        assert!(
            !plan.boundaries.is_empty(),
            "{} emitted no boundaries",
            backend.id()
        );

        assert_safe_plan(backend, fixture.formatting_source, &plan);

        let formatted =
            rewrite(fixture.formatting_source, &plan.boundaries).expect("backend plan is safe");

        assert_eq!(formatted, fixture.formatting_expected);

        let BackendResult::Ready(second_plan) = backend.plan(&formatted) else {
            panic!("{} rejected its formatted output", backend.id());
        };

        assert_eq!(
            rewrite(&formatted, &second_plan.boundaries).expect("formatted plan is safe"),
            formatted
        );
    }
}

#[test]
fn every_registered_backend_preserves_line_endings() {
    for backend in registered_backends() {
        let fixture = fixture_for(backend);
        let source = with_crlf(fixture.formatting_source);
        let path = fixture_path(fixture.extension, "line-endings");
        fs::write(&path, &source).expect("write CRLF fixture");
        let first = run_cli("format", &path);
        assert!(first.status.success(), "stderr: {:?}", first.stderr);
        let formatted = fs::read(&path).expect("read formatted CRLF fixture");
        assert!(!has_lone_lf(&formatted));
        let second = run_cli("format", &path);
        assert!(second.status.success(), "stderr: {:?}", second.stderr);

        assert_eq!(
            fs::read(&path).expect("read second formatted fixture"),
            formatted
        );

        fs::remove_file(path).expect("remove CRLF fixture");
    }
}

#[test]
fn every_registered_backend_preserves_comment_barriers() {
    for backend in registered_backends() {
        let fixture = fixture_for(backend);
        let path = fixture_path(fixture.extension, "barrier");
        fs::write(&path, fixture.barrier_source).expect("write barrier fixture");
        let output = run_cli("format", &path);
        assert!(output.status.success(), "stderr: {:?}", output.stderr);

        assert_eq!(
            fs::read(&path).expect("read formatted barrier fixture"),
            fixture.barrier_source
        );

        fs::remove_file(path).expect("remove barrier fixture");
    }
}

#[test]
fn every_registered_backend_reports_errors_without_a_rewrite_plan() {
    for backend in registered_backends() {
        let fixture = fixture_for(backend);

        let BackendResult::Diagnostics(diagnostics) = backend.plan(fixture.malformed_source) else {
            panic!("{} accepted malformed input", backend.id());
        };

        assert!(
            !diagnostics.is_empty(),
            "{} returned no diagnostics",
            backend.id()
        );

        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.backend == backend.id())
        );
    }
}

#[test]
fn explicit_file_selection_and_diagnostics_are_deterministic() {
    for backend in registered_backends() {
        let fixture = fixture_for(backend);
        let path = fixture_path(fixture.extension, "diagnostics");
        fs::write(&path, fixture.malformed_source).expect("write malformed fixture");
        let first = run_cli("check", &path);
        assert_eq!(first.status.code(), Some(2));
        assert!(first.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&first.stderr);
        assert!(stderr.contains(&path.display().to_string()));
        assert!(stderr.contains(backend.id()));
        assert!(stderr.contains("line"));
        let second = run_cli("check", &path);
        assert_eq!(first.status.code(), second.status.code());
        assert_eq!(first.stdout, second.stdout);
        assert_eq!(first.stderr, second.stderr);
        fs::remove_file(path).expect("remove malformed fixture");
    }
}

fn assert_safe_plan(backend: &dyn Backend, source: &[u8], plan: &FormatPlan) {
    for boundary in &plan.boundaries {
        assert!(boundary.range.start <= boundary.range.end);
        assert!(boundary.range.end <= source.len());

        assert!(
            boundary
                .indentation
                .iter()
                .all(|byte| matches!(byte, b' ' | b'\t')),
            "{} emitted non-whitespace indentation",
            backend.id()
        );
    }
}

fn with_crlf(source: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(source.len());

    for &byte in source {
        if byte == b'\n' {
            output.extend_from_slice(b"\r\n");
        } else {
            output.push(byte);
        }
    }

    output
}

fn has_lone_lf(source: &[u8]) -> bool {
    source
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'\n' && (index == 0 || source[index - 1] != b'\r'))
}

fn fixture_path(extension: &str, name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after epoch")
        .as_nanos();

    PathBuf::from(format!(
        "{}/glade-contract-{}-{name}-{nonce}.{extension}",
        std::env::temp_dir().display(),
        std::process::id(),
    ))
}

fn run_cli(command: &str, path: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_glade"))
        .arg(command)
        .arg(path)
        .output()
        .expect("formatter runs")
}

struct TestBackend(&'static str);

impl Backend for TestBackend {
    fn id(&self) -> &'static str {
        self.0
    }

    fn plan(&self, _source: &[u8]) -> BackendResult {
        BackendResult::Ready(FormatPlan {
            boundaries: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

static FIRST: TestBackend = TestBackend("first");
static SECOND: TestBackend = TestBackend("second");

#[test]
fn duplicate_extension_claims_are_reported() {
    let registry = [
        BackendRegistration {
            backend: &FIRST,
            extensions: &["rs"],
        },
        BackendRegistration {
            backend: &SECOND,
            extensions: &["rs"],
        },
    ];

    assert_eq!(
        validate_registry(&registry),
        Err(RegistryError::DuplicateExtension {
            extension: "rs".to_owned(),
        })
    );
}
