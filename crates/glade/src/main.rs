use std::{
    fs, io,
    path::{Path, PathBuf},
    process,
};

use clap::{Parser, Subcommand};
use thiserror::Error;
use tree_sitter::{Node, Parser as TreeSitterParser};

#[derive(Parser)]
#[command(name = "glade")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
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
struct ItemSpan {
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
    let Cli { command } = Cli::parse();
    let Command::Format { files } = command;

    let mut failed = false;
    for path in files {
        match format_file(&path) {
            Ok(true) => eprintln!("{}", path.display()),
            Ok(false) => {}
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                failed = true;
            }
        }
    }

    if failed {
        process::exit(2);
    }
}

fn format_file(path: &Path) -> Result<bool, FileError> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        return Err(FileError::UnsupportedExtension);
    }

    let source = fs::read_to_string(path)?;
    let formatted = format_source(source.as_bytes())?;
    let changed = formatted != source.as_bytes();
    if changed {
        fs::write(path, formatted)?;
    }
    Ok(changed)
}

fn format_source(source: &[u8]) -> Result<Vec<u8>, FormatError> {
    let line_ending = line_ending(source)?;
    let source_text = std::str::from_utf8(source).map_err(|_| FormatError::Parse)?;

    let mut parser = TreeSitterParser::new();

    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Rust grammar loads");

    let tree = parser
        .parse(source_text, None)
        .expect("Tree-sitter returns a tree");

    if tree.root_node().has_error() {
        return Err(FormatError::Parse);
    }

    let mut patches = Vec::new();
    format_container(source, tree.root_node(), line_ending, &mut patches);
    patches.sort_by_key(|patch| patch.start);

    let mut output = source.to_vec();

    for patch in patches.into_iter().rev() {
        output.splice(patch.start..patch.end, patch.replacement);
    }

    Ok(output)
}

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

    if has_lone_cr || (has_crlf && has_lf) {
        return Err(FormatError::MixedLineEndings);
    }

    Ok(if has_crlf { b"\r\n" } else { b"\n" })
}

fn format_container(
    source: &[u8],
    container: Node<'_>,
    line_ending: &[u8],
    patches: &mut Vec<Patch>,
) {
    let children = named_children(container);

    let item_indices: Vec<_> = children
        .iter()
        .enumerate()
        .filter_map(|(index, child)| is_item(child.kind()).then_some(index))
        .collect();

    let items: Vec<_> = item_indices
        .iter()
        .map(|index| item_span(source, &children, *index))
        .collect();

    for pair in items.windows(2) {
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
    }

    for index in item_indices {
        let item = children[index];

        if item.kind() != "mod_item" {
            continue;
        }

        if let Some(body) = named_children(item)
            .into_iter()
            .find(|child| child.kind() == "declaration_list")
        {
            format_container(source, body, line_ending, patches);
        }
    }
}

fn item_span(source: &[u8], children: &[Node<'_>], index: usize) -> ItemSpan {
    let item = children[index];
    let mut start = item.start_byte();
    let mut previous = index;

    while previous > 0 {
        let candidate = children[previous - 1];

        if !is_attachment(candidate, source)
            || (has_blank_line(&source[candidate.end_byte()..start])
                && !is_outer_attribute(candidate, source))
        {
            break;
        }

        if matches!(candidate.kind(), "line_comment" | "block_comment")
            && has_preceding_item_on_line(source, children, previous - 1)
        {
            break;
        }

        start = candidate.start_byte();
        previous -= 1;
    }

    let mut end = item.end_byte();
    let mut next = index + 1;

    while let Some(candidate) = children.get(next).copied() {
        if !is_trailing_comment(candidate) || has_line_break(&source[end..candidate.start_byte()]) {
            break;
        }

        end = candidate.end_byte();
        next += 1;
    }

    let barrier_before = previous > 0
        && start != item.start_byte()
        && matches!(children[previous].kind(), "line_comment" | "block_comment")
        && has_blank_line(&source[children[previous - 1].end_byte()..start]);

    let barrier_after = end != item.end_byte()
        && children
            .get(next)
            .is_some_and(|candidate| has_blank_line(&source[end..candidate.start_byte()]));

    ItemSpan {
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
        "const_item"
            | "enum_item"
            | "extern_crate_declaration"
            | "foreign_mod_item"
            | "function_item"
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

fn has_preceding_item_on_line(source: &[u8], children: &[Node<'_>], comment_index: usize) -> bool {
    let mut current = comment_index;
    while current > 0 {
        let previous = children[current - 1];
        let comment = children[current];
        if !is_trailing_comment(comment)
            || has_line_break(&source[previous.end_byte()..comment.start_byte()])
        {
            return false;
        }
        if is_item(previous.kind()) {
            return true;
        }
        if !matches!(previous.kind(), "line_comment" | "block_comment") {
            return false;
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
