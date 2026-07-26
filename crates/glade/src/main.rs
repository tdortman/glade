use std::{
    fs, io,
    path::{Path, PathBuf},
    process,
};

use clap::{ArgAction, Parser, Subcommand};
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
}

#[derive(Debug, Error)]
enum FormatError {
    #[error("Rust source contains syntax errors")]
    Parse,

    #[error("source mixes LF and CRLF line endings")]
    MixedLineEndings,
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

fn main() {
    let Cli { command, verbose } = Cli::parse();
    init_tracing(verbose);
    debug!(?command, "parsed command");
    let Command::Format { files } = command;
    info!(files = files.len(), "formatting input files");
    let mut failed = false;

    for path in files {
        match format_file(&path) {
            Ok(true) => eprintln!("{}", path.display()),
            Ok(false) => {}

            Err(error) => {
                error!(path = %path.display(), error = %error, "formatting failed");
                eprintln!("{}: {error}", path.display());
                failed = true;
            }
        }
    }

    if failed {
        warn!("one or more files failed to format");
        process::exit(2);
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

#[instrument(skip_all, fields(path = %path.display()))]
fn format_file(path: &Path) -> Result<bool, FileError> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        warn!("unsupported file extension");
        return Err(FileError::UnsupportedExtension);
    }

    debug!("reading source");
    let source = fs::read_to_string(path)?;
    debug!(bytes = source.len(), "source read");
    let formatted = format_source(source.as_bytes())?;
    let changed = formatted != source.as_bytes();

    debug!(
        changed,
        bytes_before = source.len(),
        bytes_after = formatted.len(),
        "formatting complete"
    );

    if changed {
        info!("writing formatted source");
        fs::write(path, formatted)?;
    }

    Ok(changed)
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
        FormatError::Parse
    })?;

    let mut parser = TreeSitterParser::new();

    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Rust grammar loads");

    trace!("parsing source with tree-sitter");

    let tree = parser
        .parse(source_text, None)
        .expect("Tree-sitter returns a tree");

    if tree.root_node().has_error() {
        warn!("tree-sitter reported syntax errors");
        return Err(FormatError::Parse);
    }

    let mut patches = Vec::new();
    format_container(source, tree.root_node(), line_ending, &mut patches);
    debug!(patches = patches.len(), "generated formatting patches");
    patches.sort_by_key(|patch| patch.start);
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
) {
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

        if !gap.iter().all(u8::is_ascii_whitespace) {
            continue;
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
        );
    }
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
) {
    if is_atomic(node.kind()) {
        trace!("skipping atomic node");
        return;
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
                format_container(source, child, line_ending, patches);
            } else {
                trace!(child_kind = child.kind(), "descending into nested node");
                format_nested_containers(source, child, line_ending, patches, false, false);
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
            );
        }
    }
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
