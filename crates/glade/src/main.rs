use std::{
    borrow::Cow,
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process,
};

use clap::{ArgAction, Parser, Subcommand};
use similar::TextDiff;
use thiserror::Error;
use tracing::{debug, error, info, instrument, trace, warn};
use tracing_subscriber::filter::LevelFilter;
use tree_sitter::{Node, Parser as TreeSitterParser};

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

#[derive(Debug, Error)]
enum FormatError {
    #[error("Rust source contains syntax errors at line {line}, column {column}")]
    Parse { line: usize, column: usize },

    #[error("Rust source is not valid UTF-8")]
    InvalidUtf8,

    #[error("Rust parser returned an incomplete syntax tree")]
    MissingStructure,

    #[error("source mixes LF and CRLF line endings")]
    MixedLineEndings,

    #[error("formatting patches overlap or conflict")]
    PatchConflict,

    #[error("editable whitespace range contains non-whitespace bytes")]
    UnsafeRewrite,
}

#[derive(Debug, Error)]
enum FileError {
    #[error("unsupported file extension; expected .rs")]
    UnsupportedExtension,

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Format(#[from] FormatError),
}

#[derive(Clone, Copy)]
struct BoundarySpan {
    start: usize,
    end: usize,
    multiline: bool,
    barrier_before: bool,
    barrier_after: bool,
}

struct Patch {
    start: usize,
    end: usize,
    replacement: Vec<u8>,
}

enum ProcessResult {
    Success,
    Drift(String),
}

fn main() {
    let Cli { command, verbose } = Cli::parse();
    init_tracing(verbose);
    debug!(?command, "parsed command");
    process::exit(run(command));
}

fn run(command: Command) -> i32 {
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

#[instrument(skip_all, fields(path = %path.display(), ?mode))]
fn process_file(path: &Path, mode: Mode) -> Result<ProcessResult, FileError> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        warn!("unsupported file extension");
        return Err(FileError::UnsupportedExtension);
    }

    debug!("reading source");
    let source = fs::read(path)?;
    debug!(bytes = source.len(), "source read");
    let formatted = format_source(&source)?;
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

#[instrument(skip(source), fields(bytes = source.len()))]
fn format_source(source: &[u8]) -> Result<Vec<u8>, FormatError> {
    let line_ending = line_ending(source)?;

    debug!(
        style = if line_ending == b"\r\n" { "CRLF" } else { "LF" },
        "detected line ending"
    );

    let source_text = std::str::from_utf8(source).map_err(|_| {
        warn!("source is not valid UTF-8");
        FormatError::InvalidUtf8
    })?;

    let mut parser = TreeSitterParser::new();

    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Rust grammar loads");

    trace!("parsing source with tree-sitter");

    let tree = parser
        .parse(source_text, None)
        .expect("Tree-sitter returns a tree");

    let root = tree.root_node();

    if root.kind() != "source_file" || has_missing_node(root) {
        warn!("tree-sitter returned an incomplete syntax tree");
        return Err(FormatError::MissingStructure);
    }

    if root.has_error() {
        let position = first_error_position(root).unwrap_or_else(|| root.start_position());
        let line = position.row + 1;
        let column = position.column + 1;
        warn!(line, column, "tree-sitter reported syntax errors");
        return Err(FormatError::Parse { line, column });
    }

    let mut patches = Vec::new();
    format_container(source, root, line_ending, &mut patches)?;
    debug!(patches = patches.len(), "generated formatting patches");
    validate_patches(source, &mut patches)?;
    let mut output = source.to_vec();

    for patch in patches.into_iter().rev() {
        trace!(
            start = patch.start,
            end = patch.end,
            replacement_bytes = patch.replacement.len(),
            "applying formatting patch"
        );

        output.splice(patch.start..patch.end, patch.replacement);
    }

    Ok(output)
}

fn has_missing_node(node: Node<'_>) -> bool {
    if node.is_missing() {
        return true;
    }

    let mut cursor = node.walk();
    node.children(&mut cursor).any(has_missing_node)
}

fn first_error_position(node: Node<'_>) -> Option<tree_sitter::Point> {
    if node.kind() == "ERROR" {
        return Some(node.start_position());
    }

    let mut cursor = node.walk();
    node.children(&mut cursor).find_map(first_error_position)
}

fn validate_patches(source: &[u8], patches: &mut Vec<Patch>) -> Result<(), FormatError> {
    patches.sort_by_key(|patch| (patch.start, patch.end));
    let mut validated: Vec<Patch> = Vec::with_capacity(patches.len());

    for patch in patches.drain(..) {
        let Some(range) = source.get(patch.start..patch.end) else {
            return Err(FormatError::UnsafeRewrite);
        };

        if !range.iter().all(|byte| is_editable_whitespace(*byte)) {
            return Err(FormatError::UnsafeRewrite);
        }

        if let Some(previous) = validated.last() {
            if previous.start == patch.start && previous.end == patch.end {
                if previous.replacement == patch.replacement {
                    continue;
                }

                return Err(FormatError::PatchConflict);
            }

            if previous.end > patch.start {
                return Err(FormatError::PatchConflict);
            }
        }

        validated.push(patch);
    }

    *patches = validated;
    Ok(())
}

#[instrument(skip(source), fields(bytes = source.len()))]
fn line_ending(source: &[u8]) -> Result<&'static [u8], FormatError> {
    let has_crlf = source.windows(2).any(|pair| pair == b"\r\n");

    let has_lf = source
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'\n' && (index == 0 || source[index - 1] != b'\r'));

    let has_lone_cr = source
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'\r' && source.get(index + 1) != Some(&b'\n'));

    trace!(has_crlf, has_lf, has_lone_cr, "inspected line endings");

    if has_lone_cr || (has_crlf && has_lf) {
        warn!("mixed line endings detected");
        return Err(FormatError::MixedLineEndings);
    }

    Ok(if has_crlf { b"\r\n" } else { b"\n" })
}

#[instrument(
    skip(source, line_ending, patches),
    fields(
        kind = container.kind(),
        start = container.start_byte(),
        end = container.end_byte()
    )
)]
fn format_container(
    source: &[u8],
    container: Node<'_>,
    line_ending: &[u8],
    patches: &mut Vec<Patch>,
) -> Result<(), FormatError> {
    let children = named_children(container);
    let separator = separator_for(container.kind());
    let attach_visibility = container.kind() == "ordered_field_declaration_list";

    let eligible_indices: Vec<_> = children
        .iter()
        .enumerate()
        .filter_map(|(index, child)| {
            (is_eligible_child(container.kind(), child.kind())
                || (container.kind() == "block" && is_block_tail_expression(child.kind())))
            .then_some(index)
        })
        .collect();

    trace!(
        children = children.len(),
        eligible = eligible_indices.len(),
        separator = ?separator,
        attach_visibility,
        "analysing container"
    );

    let boundary_spans: Vec<_> = eligible_indices
        .iter()
        .map(|index| {
            boundary_span(
                source,
                &children,
                *index,
                separator,
                attach_visibility,
                &eligible_indices,
            )
        })
        .collect();

    for pair in boundary_spans.windows(2) {
        let [previous, next] = pair else {
            unreachable!()
        };

        if previous.barrier_after || next.barrier_before {
            continue;
        }

        let gap = &source[previous.end..next.start];

        if !gap.iter().all(|byte| is_editable_whitespace(*byte)) {
            warn!(
                start = previous.end,
                end = next.start,
                "editable whitespace range contains non-whitespace bytes"
            );

            return Err(FormatError::UnsafeRewrite);
        }

        let required = previous.multiline || next.multiline;

        if !required && !has_line_break(gap) {
            continue;
        }

        let indent = line_indent(source, next.start);
        let mut replacement = Vec::new();
        replacement.extend_from_slice(line_ending);

        if required {
            replacement.extend_from_slice(line_ending);
        }

        replacement.extend_from_slice(indent);

        patches.push(Patch {
            start: previous.end,
            end: next.start,
            replacement,
        });

        trace!(
            start = previous.end,
            end = next.start,
            required,
            "scheduled whitespace patch"
        );
    }

    for child in children {
        format_nested_containers(
            source,
            child,
            line_ending,
            patches,
            container.kind() == "block",
            true,
        )?;
    }

    Ok(())
}

#[instrument(
    skip(source, line_ending, patches),
    fields(
        kind = node.kind(),
        start = node.start_byte(),
        end = node.end_byte(),
        allow_expression_bodies,
        allow_structural_bodies
    )
)]
fn format_nested_containers(
    source: &[u8],
    node: Node<'_>,
    line_ending: &[u8],
    patches: &mut Vec<Patch>,
    allow_expression_bodies: bool,
    allow_structural_bodies: bool,
) -> Result<(), FormatError> {
    if is_atomic(node.kind()) {
        trace!("skipping atomic node");
        return Ok(());
    }

    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        if is_container(child.kind()) {
            if is_container_body(node, child)
                && (allow_expression_bodies
                    || node.kind() == "closure_expression"
                    || (allow_structural_bodies && is_structural_body(node.kind())))
            {
                trace!(child_kind = child.kind(), "formatting nested container");
                format_container(source, child, line_ending, patches)?;
            } else {
                trace!(child_kind = child.kind(), "descending into nested node");
                format_nested_containers(source, child, line_ending, patches, false, false)?;
            }
        } else if !is_atomic(child.kind()) {
            let child_allows_bodies = allow_expression_bodies
                && ((node.kind() == "expression_statement" && is_block_expression(child.kind()))
                    || (node.kind() == "if_expression" && child.kind() == "else_clause")
                    || (node.kind() == "else_clause" && child.kind() == "if_expression"));

            format_nested_containers(
                source,
                child,
                line_ending,
                patches,
                child_allows_bodies,
                allow_structural_bodies && is_structural_body(node.kind()),
            )?;
        }
    }

    Ok(())
}

fn is_atomic(kind: &str) -> bool {
    matches!(kind, "macro_definition" | "macro_invocation" | "token_tree")
}

fn is_structural_body(kind: &str) -> bool {
    matches!(
        kind,
        "closure_expression"
            | "enum_item"
            | "enum_variant"
            | "foreign_mod_item"
            | "function_item"
            | "impl_item"
            | "mod_item"
            | "struct_item"
            | "trait_item"
            | "union_item"
    )
}

fn is_block_expression(kind: &str) -> bool {
    matches!(
        kind,
        "async_block"
            | "const_block"
            | "for_expression"
            | "gen_block"
            | "if_expression"
            | "loop_expression"
            | "match_expression"
            | "try_block"
            | "unsafe_block"
            | "while_expression"
    )
}

fn is_container(kind: &str) -> bool {
    matches!(
        kind,
        "block"
            | "declaration_list"
            | "enum_variant_list"
            | "field_declaration_list"
            | "match_block"
            | "ordered_field_declaration_list"
    )
}

fn separator_for(kind: &str) -> Option<u8> {
    match kind {
        "source_file" | "declaration_list" => Some(b';'),

        "enum_variant_list" | "field_declaration_list" | "ordered_field_declaration_list" => {
            Some(b',')
        }

        _ => None,
    }
}

fn is_eligible_child(container: &str, child: &str) -> bool {
    match container {
        "source_file" | "declaration_list" => is_item(child),

        "block" => {
            (is_item(child)
                || matches!(
                    child,
                    "empty_statement" | "expression_statement" | "let_declaration"
                ))
                && !matches!(child, "attribute_item" | "inner_attribute_item")
        }

        "enum_variant_list" => child == "enum_variant",
        "field_declaration_list" => child == "field_declaration",
        "match_block" => child == "match_arm",

        "ordered_field_declaration_list" => !matches!(
            child,
            "attribute_item"
                | "block_comment"
                | "inner_attribute_item"
                | "line_comment"
                | "visibility_modifier"
        ),

        _ => false,
    }
}

fn is_block_tail_expression(kind: &str) -> bool {
    !is_item(kind)
        && !matches!(
            kind,
            "attribute_item"
                | "block_comment"
                | "empty_statement"
                | "inner_attribute_item"
                | "let_declaration"
                | "line_comment"
                | "expression_statement"
        )
}

fn is_container_body(parent: Node<'_>, child: Node<'_>) -> bool {
    match child.kind() {
        "block" => match parent.kind() {
            "async_block" | "const_block" | "else_clause" | "gen_block" | "loop_expression"
            | "try_block" | "unsafe_block" | "while_expression" => true,

            "closure_expression" | "for_expression" | "function_item" => {
                has_child_field(parent, "body", child)
            }

            "if_expression" => has_child_field(parent, "consequence", child),
            "let_declaration" => has_child_field(parent, "alternative", child),
            _ => false,
        },

        "declaration_list" => {
            matches!(
                parent.kind(),
                "foreign_mod_item" | "impl_item" | "mod_item" | "trait_item"
            ) && has_child_field(parent, "body", child)
        }

        "enum_variant_list" => {
            parent.kind() == "enum_item" && has_child_field(parent, "body", child)
        }

        "field_declaration_list" | "ordered_field_declaration_list" => {
            matches!(parent.kind(), "enum_variant" | "struct_item" | "union_item")
                && has_child_field(parent, "body", child)
        }

        "match_block" => {
            parent.kind() == "match_expression" && has_child_field(parent, "body", child)
        }

        _ => false,
    }
}

fn has_child_field(parent: Node<'_>, field: &str, child: Node<'_>) -> bool {
    parent
        .child_by_field_name(field)
        .is_some_and(|candidate| candidate.start_byte() == child.start_byte())
}

fn boundary_span(
    source: &[u8],
    children: &[Node<'_>],
    index: usize,
    separator: Option<u8>,
    attach_visibility: bool,
    eligible: &[usize],
) -> BoundarySpan {
    let construct = children[index];
    let mut start = construct.start_byte();
    let mut previous = index;

    while previous > 0 {
        let candidate = children[previous - 1];

        if !(is_attachment(candidate, source)
            || (attach_visibility && candidate.kind() == "visibility_modifier"))
            || (has_blank_line(&source[candidate.end_byte()..start])
                && !is_outer_attribute(candidate, source)
                && !(attach_visibility && candidate.kind() == "visibility_modifier"))
        {
            break;
        }

        if matches!(candidate.kind(), "line_comment" | "block_comment")
            && has_preceding_eligible_on_line(source, children, previous - 1, eligible)
        {
            break;
        }

        start = candidate.start_byte();
        previous -= 1;
    }

    let mut end = construct.end_byte();
    let mut next = index + 1;

    if let Some(separator) = separator {
        let limit = children.get(next).map_or(source.len(), Node::start_byte);
        let mut separator_start = end;

        while separator_start < limit && source[separator_start].is_ascii_whitespace() {
            separator_start += 1;
        }

        if source.get(separator_start) == Some(&separator) {
            end = separator_start + 1;
        }
    }

    while let Some(candidate) = children.get(next).copied() {
        if !is_trailing_comment(candidate) || has_line_break(&source[end..candidate.start_byte()]) {
            break;
        }

        end = candidate.end_byte();
        next += 1;
    }

    if let Some(separator) = separator {
        let limit = children.get(next).map_or(source.len(), Node::start_byte);
        let mut separator_start = end;

        while separator_start < limit && source[separator_start].is_ascii_whitespace() {
            separator_start += 1;
        }

        if source.get(separator_start) == Some(&separator) {
            end = separator_start + 1;
        }
    }

    let barrier_before = previous > 0
        && start != construct.start_byte()
        && matches!(children[previous].kind(), "line_comment" | "block_comment")
        && has_blank_line(&source[children[previous - 1].end_byte()..start]);

    let barrier_after = next > index + 1
        && children
            .get(next)
            .is_some_and(|candidate| has_blank_line(&source[end..candidate.start_byte()]));

    BoundarySpan {
        start,
        end,
        multiline: has_line_break(&source[start..end]),
        barrier_before,
        barrier_after,
    }
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn is_item(kind: &str) -> bool {
    matches!(
        kind,
        "associated_type"
            | "const_item"
            | "enum_item"
            | "extern_crate_declaration"
            | "foreign_mod_item"
            | "function_item"
            | "function_signature_item"
            | "impl_item"
            | "macro_definition"
            | "macro_invocation"
            | "mod_item"
            | "static_item"
            | "struct_item"
            | "trait_item"
            | "type_item"
            | "union_item"
            | "use_declaration"
    )
}

fn is_attachment(node: Node<'_>, source: &[u8]) -> bool {
    match node.kind() {
        "line_comment" | "block_comment" => true,
        "attribute_item" => is_outer_attribute(node, source),
        _ => false,
    }
}

fn is_outer_attribute(node: Node<'_>, source: &[u8]) -> bool {
    node.kind() == "attribute_item"
        && !source[node.start_byte()..node.end_byte()].starts_with(b"#![")
}

fn is_trailing_comment(node: Node<'_>) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

fn has_preceding_eligible_on_line(
    source: &[u8],
    children: &[Node<'_>],
    comment_index: usize,
    eligible: &[usize],
) -> bool {
    let mut current = comment_index;

    while current > 0 {
        let previous = children[current - 1];
        let comment = children[current];

        if !is_trailing_comment(comment)
            || has_line_break(&source[previous.end_byte()..comment.start_byte()])
        {
            return false;
        }

        if !matches!(previous.kind(), "line_comment" | "block_comment") {
            return eligible.contains(&(current - 1));
        }

        current -= 1;
    }

    false
}

fn has_line_break(bytes: &[u8]) -> bool {
    bytes.iter().any(|byte| matches!(byte, b'\r' | b'\n'))
}

const fn is_editable_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn has_blank_line(bytes: &[u8]) -> bool {
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }

        let mut next = index + 1;

        while next < bytes.len() && matches!(bytes[next], b' ' | b'\t' | b'\r') {
            next += 1;
        }

        if next < bytes.len() && bytes[next] == b'\n' {
            return true;
        }
    }

    false
}

fn line_indent(source: &[u8], position: usize) -> &[u8] {
    let start = source[..position]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);

    let end = source[start..position]
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t'))
        .map_or(position, |index| start + index);

    &source[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_patches_are_deduplicated() {
        let mut patches = vec![
            Patch {
                start: 0,
                end: 1,
                replacement: b"\n".to_vec(),
            },
            Patch {
                start: 0,
                end: 1,
                replacement: b"\n".to_vec(),
            },
        ];

        validate_patches(b" ", &mut patches).expect("identical patches are safe");
        assert_eq!(patches.len(), 1);
    }

    #[test]
    fn overlapping_patches_are_rejected() {
        let mut patches = vec![
            Patch {
                start: 0,
                end: 2,
                replacement: b"\n".to_vec(),
            },
            Patch {
                start: 1,
                end: 2,
                replacement: b"\n".to_vec(),
            },
        ];

        assert!(matches!(
            validate_patches(b"  ", &mut patches),
            Err(FormatError::PatchConflict)
        ));
    }

    #[test]
    fn conflicting_insertions_are_rejected() {
        let mut patches = vec![
            Patch {
                start: 1,
                end: 1,
                replacement: b"a".to_vec(),
            },
            Patch {
                start: 1,
                end: 1,
                replacement: b"b".to_vec(),
            },
        ];

        assert!(matches!(
            validate_patches(b" ", &mut patches),
            Err(FormatError::PatchConflict)
        ));
    }

    #[test]
    fn non_editable_whitespace_is_rejected() {
        let mut patches = vec![Patch {
            start: 0,
            end: 1,
            replacement: b"\n".to_vec(),
        }];

        assert!(matches!(
            validate_patches(b"\x0b", &mut patches),
            Err(FormatError::UnsafeRewrite)
        ));
    }

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
