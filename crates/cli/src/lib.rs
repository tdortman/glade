use backend_rust::RustBackend;
use clap::{ArgAction, Parser, Subcommand};
use glade_core::{Backend, BackendResult, Diagnostic, RewriteError, rewrite};
use similar::TextDiff;

use std::{
    borrow::Cow,
    collections::HashSet,
    fmt,
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process,
};

use thiserror::Error;
use tracing::{debug, error, info, instrument, trace, warn};
use tracing_subscriber::filter::LevelFilter;

#[derive(Parser)]
#[command(name = "glade")]
struct Cli {
    #[arg(
        short = 'v',
        long,
        global = true,
        action = ArgAction::Count,
        help = "enable verbose logging, repeat for more detail"
    )]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Format {
        #[arg(value_name = "FILE", required = true)]
        files: Vec<PathBuf>,
    },

    Check {
        #[arg(value_name = "FILE", required = true)]
        files: Vec<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Format,
    Check,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum RegistryError {
    #[error("duplicate backend extension claim: .{extension}")]
    DuplicateExtension { extension: String },
}

#[derive(Clone, Copy)]
pub struct BackendRegistration {
    pub backend: &'static dyn Backend,
    pub extensions: &'static [&'static str],
}

static RUST_BACKEND: RustBackend = RustBackend;

static BACKEND_REGISTRY: &[BackendRegistration] = &[BackendRegistration {
    backend: &RUST_BACKEND,
    extensions: &["rs"],
}];

#[must_use]
pub fn backend_registry() -> &'static [BackendRegistration] {
    BACKEND_REGISTRY
}

/// Checks that every registered extension has exactly one backend.
///
/// # Errors
///
/// Returns an error when multiple registrations claim the same extension.
pub fn validate_registry(registry: &[BackendRegistration]) -> Result<(), RegistryError> {
    let mut claims = HashSet::new();

    for registration in registry {
        for &extension in registration.extensions {
            if !claims.insert(extension) {
                return Err(RegistryError::DuplicateExtension {
                    extension: extension.to_owned(),
                });
            }
        }
    }

    Ok(())
}

#[must_use]
pub fn backend_for_extension(extension: &str) -> Option<&'static dyn Backend> {
    BACKEND_REGISTRY
        .iter()
        .find(|registration| registration.extensions.contains(&extension))
        .map(|registration| registration.backend)
}

#[derive(Debug, Error)]
enum FileError {
    #[error("unsupported file extension; expected {expected}")]
    UnsupportedExtension { expected: String },

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Diagnostics(BackendDiagnostics),

    #[error(transparent)]
    Rewrite(#[from] RewriteError),
}

#[derive(Debug)]
struct BackendDiagnostics {
    source: Vec<u8>,
    diagnostics: Vec<Diagnostic>,
}

impl fmt::Display for BackendDiagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                formatter.write_str("\n")?;
            }

            formatter.write_str(&diagnostic_text(&self.source, diagnostic))?;
        }

        Ok(())
    }
}

impl std::error::Error for BackendDiagnostics {}

fn diagnostic_text(source: &[u8], diagnostic: &Diagnostic) -> String {
    let location = diagnostic.range.as_ref().map_or_else(String::new, |range| {
        let (line, column) = byte_position(source, range.start);
        format!(" at line {line}, column {column}")
    });

    let severity = match diagnostic.severity {
        glade_core::Severity::Error => "error",
        glade_core::Severity::Warning => "warning",
    };

    format!(
        "{}: {severity}: {}{location}",
        diagnostic.backend, diagnostic.message
    )
}

fn report_diagnostics(path: &Path, source: &[u8], diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        warn!(
            backend = diagnostic.backend,
            ?diagnostic.severity,
            range = ?diagnostic.range,
            message = %diagnostic.message,
            "backend diagnostic"
        );

        eprintln!(
            "{}: {}",
            path.display(),
            diagnostic_text(source, diagnostic)
        );
    }
}

fn byte_position(source: &[u8], byte: usize) -> (usize, usize) {
    let byte = byte.min(source.len());

    let line_start = source[..byte]
        .iter()
        .rposition(|value| *value == b'\n')
        .map_or(0, |position| position + 1);

    let mut line = 1;

    for value in &source[..byte] {
        line += usize::from(*value == b'\n');
    }

    (line, byte - line_start + 1)
}

enum ProcessResult {
    Success,
    Drift(String),
}

pub fn run() -> i32 {
    let Cli { command, verbose } = Cli::parse();
    init_tracing(verbose);
    debug!(?command, "parsed command");

    if let Err(error) = validate_registry(backend_registry()) {
        error!(error = %error, "backend registry validation failed");
        eprintln!("backend registry error: {error}");
        return 2;
    }

    run_command(command)
}

fn run_command(command: Command) -> i32 {
    let (mode, files) = match command {
        Command::Format { files } => (Mode::Format, files),
        Command::Check { files } => (Mode::Check, files),
    };

    info!(files = files.len(), "formatting input files");
    let mut failed = false;
    let mut drift = false;

    for path in files {
        match process_file(&path, mode) {
            Ok(ProcessResult::Drift(diff)) => {
                print_diff(&diff);
                drift = true;
            }

            Ok(ProcessResult::Success) => {}

            Err(error) => {
                error!(path = %path.display(), error = %error, "formatting failed");
                eprintln!("{}: {error}", path.display());
                failed = true;
            }
        }
    }

    if failed {
        warn!("one or more files failed to format");
        2
    } else if mode == Mode::Check && drift {
        1
    } else if mode == Mode::Check {
        print_check_success();
        0
    } else {
        0
    }
}

fn init_tracing(verbosity: u8) {
    let max_level = match verbosity {
        0 => LevelFilter::OFF,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    };

    tracing_subscriber::fmt()
        .with_max_level(max_level)
        .with_writer(std::io::stderr)
        .with_target(false)
        .compact()
        .init();
}

fn unsupported_extension() -> FileError {
    let mut extensions: Vec<_> = backend_registry()
        .iter()
        .flat_map(|registration| registration.extensions)
        .copied()
        .collect();

    extensions.sort_unstable();
    extensions.dedup();

    let expected = extensions
        .iter()
        .map(|extension| format!(".{extension}"))
        .collect::<Vec<_>>()
        .join(", ");

    FileError::UnsupportedExtension { expected }
}

#[instrument(skip_all, fields(path = %path.display(), ?mode))]
fn process_file(path: &Path, mode: Mode) -> Result<ProcessResult, FileError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .ok_or_else(unsupported_extension)?;

    let backend = backend_for_extension(extension).ok_or_else(unsupported_extension)?;
    debug!(backend = backend.id(), "selected backend");
    debug!("reading source");
    let source = fs::read(path)?;
    debug!(bytes = source.len(), "source read");

    let plan = match backend.plan(&source) {
        BackendResult::Ready(plan) => {
            if plan
                .diagnostics
                .iter()
                .any(|diagnostic| matches!(diagnostic.severity, glade_core::Severity::Error))
            {
                return Err(FileError::Diagnostics(BackendDiagnostics {
                    source,
                    diagnostics: plan.diagnostics,
                }));
            }

            report_diagnostics(path, &source, &plan.diagnostics);
            plan
        }
        BackendResult::Diagnostics(diagnostics) => {
            for diagnostic in &diagnostics {
                warn!(
                    backend = diagnostic.backend,
                    ?diagnostic.severity,
                    range = ?diagnostic.range,
                    message = %diagnostic.message,
                    "backend diagnostic"
                );
            }

            return Err(FileError::Diagnostics(BackendDiagnostics {
                source,
                diagnostics,
            }));
        }
    };

    trace!(
        boundaries = plan.boundaries.len(),
        "applying formatting patch"
    );

    let formatted = rewrite(&source, &plan.boundaries)?;
    let changed = formatted != source;

    debug!(
        changed,
        bytes_before = source.len(),
        bytes_after = formatted.len(),
        "formatting complete"
    );

    if !changed {
        return Ok(ProcessResult::Success);
    }

    if mode == Mode::Format {
        let permissions = fs::metadata(path)?.permissions();
        info!("atomically replacing formatted source");
        replace_file(path, &formatted, permissions)?;
        return Ok(ProcessResult::Success);
    }

    Ok(ProcessResult::Drift(render_diff(path, &source, &formatted)))
}

fn render_diff(path: &Path, source: &[u8], formatted: &[u8]) -> String {
    let source = normalise_line_endings(std::str::from_utf8(source).expect("source is UTF-8"));

    let formatted =
        normalise_line_endings(std::str::from_utf8(formatted).expect("formatted source is UTF-8"));

    let path = path.display().to_string().replace('\\', "/");

    TextDiff::from_lines(&source, &formatted)
        .unified_diff()
        .header(&path, &path)
        .to_string()
}

fn print_diff(diff: &str) {
    if should_colour_output() {
        print!("{}", colour_diff(diff));
    } else {
        print!("{diff}");
    }
}

fn colour_diff(diff: &str) -> String {
    const BOLD: &str = "\x1b[1m";
    const CYAN: &str = "\x1b[36m";
    const GREEN: &str = "\x1b[32m";
    const RED: &str = "\x1b[31m";
    const RESET: &str = "\x1b[0m";
    let mut coloured = String::with_capacity(diff.len());

    for line in diff.split_inclusive('\n') {
        let colour = if line.starts_with("---") || line.starts_with("+++") {
            BOLD
        } else if line.starts_with('+') {
            GREEN
        } else if line.starts_with('-') {
            RED
        } else if line.starts_with("@@") {
            CYAN
        } else {
            ""
        };

        if colour.is_empty() {
            coloured.push_str(line);
        } else {
            coloured.push_str(colour);
            coloured.push_str(line);
            coloured.push_str(RESET);
        }
    }

    coloured
}

fn should_colour_output() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn normalise_line_endings(source: &str) -> Cow<'_, str> {
    if source.contains('\r') {
        Cow::Owned(source.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(source)
    }
}

fn print_check_success() {
    const MESSAGE: &str = "All checks passed!";

    if should_colour_output() {
        println!("\x1b[32m{MESSAGE}\x1b[0m");
    } else {
        println!("{MESSAGE}");
    }
}

#[instrument(skip(contents), fields(path = %path.display(), bytes = contents.len()))]
fn replace_file(path: &Path, contents: &[u8], permissions: fs::Permissions) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));

    let name = path.file_name().map_or_else(
        || "source".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );

    for attempt in 0..100 {
        let temporary = parent.join(format!(".{name}.glade-{}-{attempt}.tmp", process::id()));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            options.mode(0o600);
        }

        let file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let result = (|| {
            let mut file = file;
            file.write_all(contents)?;
            file.set_permissions(permissions)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }

        return result;
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique temporary file",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_diff_marks_headers_hunks_and_changes() {
        let coloured = colour_diff("--- path\n+++ path\n@@ -1 +1 @@\n context\n-old\n+new\n");
        assert!(coloured.contains("\x1b[1m--- path\n\x1b[0m"));
        assert!(coloured.contains("\x1b[1m+++ path\n\x1b[0m"));
        assert!(coloured.contains("\x1b[36m@@ -1 +1 @@\n\x1b[0m"));
        assert!(coloured.contains(" context\n"));
        assert!(coloured.contains("\x1b[31m-old\n\x1b[0m"));
        assert!(coloured.contains("\x1b[32m+new\n\x1b[0m"));
    }
}
