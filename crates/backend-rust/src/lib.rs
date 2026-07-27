use glade_core::{Backend, BackendResult, Boundary, Diagnostic, FormatPlan, LineEnding, Severity};
use std::ops::Range;
use tracing::{debug, instrument, trace, warn};
use tree_sitter::{Node, Parser as TreeSitterParser};

pub struct RustBackend;
const BACKEND_ID: &str = "rust";

impl Backend for RustBackend {
    fn id(&self) -> &'static str {
        BACKEND_ID
    }

    fn plan(&self, source: &[u8]) -> BackendResult {
        plan_source(source)
    }
}

#[instrument(skip(source), fields(bytes = source.len()))]
fn plan_source(source: &[u8]) -> BackendResult {
    let Some(line_ending) = line_ending(source) else {
        return BackendResult::Diagnostics(vec![diagnostic(
            "source mixes LF and CRLF line endings",
            None,
        )]);
    };

    debug!(
        style = if line_ending == LineEnding::CrLf {
            "CRLF"
        } else {
            "LF"
        },
        "detected line ending"
    );

    let Ok(source_text) = std::str::from_utf8(source) else {
        warn!("source is not valid UTF-8");

        return BackendResult::Diagnostics(vec![diagnostic(
            "Rust source is not valid UTF-8",
            None,
        )]);
    };

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

        return BackendResult::Diagnostics(vec![diagnostic(
            "Rust parser returned an incomplete syntax tree",
            None,
        )]);
    }

    if root.has_error() {
        let error = first_error_node(root);
        let position = error.map_or_else(|| root.start_position(), |node| node.start_position());
        let line = position.row + 1;
        let column = position.column + 1;
        let range = error.map(|node| node.start_byte()..node.end_byte());
        warn!(line, column, "tree-sitter reported syntax errors");

        return BackendResult::Diagnostics(vec![diagnostic(
            "Rust source contains syntax errors",
            range,
        )]);
    }

    let mut boundaries = Vec::new();
    format_container(source, root, line_ending, &mut boundaries);

    debug!(
        boundaries = boundaries.len(),
        "generated formatting boundaries"
    );

    BackendResult::Ready(FormatPlan {
        boundaries,
        diagnostics: Vec::new(),
    })
}

fn diagnostic(message: impl Into<String>, range: Option<Range<usize>>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        range,
        message: message.into(),
        backend: BACKEND_ID,
    }
}

fn has_missing_node(node: Node<'_>) -> bool {
    if node.is_missing() {
        return true;
    }

    let mut cursor = node.walk();
    node.children(&mut cursor).any(has_missing_node)
}

fn first_error_node(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "ERROR" {
        return Some(node);
    }

    let mut cursor = node.walk();
    node.children(&mut cursor).find_map(first_error_node)
}

#[instrument(skip(source, line_ending, boundaries), fields(
    kind = container.kind(),
    start = container.start_byte(),
    end = container.end_byte()
))]
fn format_container(
    source: &[u8],
    container: Node<'_>,
    line_ending: LineEnding,
    boundaries: &mut Vec<Boundary>,
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

    for (pair_index, pair) in boundary_spans.windows(2).enumerate() {
        let [previous, next] = pair else {
            unreachable!()
        };

        let previous_child = children[eligible_indices[pair_index]];
        let next_child = children[eligible_indices[pair_index + 1]];

        let import_subgroup_barrier = previous_child.kind() == "use_declaration"
            && next_child.kind() == "use_declaration"
            && has_blank_line(&source[previous.end..next.start]);

        let force_after =
            previous_child.kind() == "use_declaration" && next_child.kind() != "use_declaration";

        let range = previous.end..next.start;
        let required = previous.multiline || next.multiline || force_after;
        let barrier = previous.barrier_after || next.barrier_before || import_subgroup_barrier;

        boundaries.push(Boundary {
            range,
            required,
            indentation: line_indent(source, next.start).to_vec(),
            line_ending,
            barrier,
        });

        trace!(
            start = previous.end,
            end = next.start,
            required,
            barrier,
            "scheduled formatting boundary"
        );
    }

    for child in children {
        format_nested_containers(
            source,
            child,
            line_ending,
            boundaries,
            container.kind() == "block",
            true,
        );
    }
}

#[instrument(skip(source, line_ending, boundaries), fields(
    kind = node.kind(),
    start = node.start_byte(),
    end = node.end_byte(),
    allow_expression_bodies,
    allow_structural_bodies
))]
fn format_nested_containers(
    source: &[u8],
    node: Node<'_>,
    line_ending: LineEnding,
    boundaries: &mut Vec<Boundary>,
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
                format_container(source, child, line_ending, boundaries);
            } else {
                trace!(child_kind = child.kind(), "descending into nested node");
                format_nested_containers(source, child, line_ending, boundaries, false, false);
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
                boundaries,
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

#[derive(Clone, Copy)]
struct BoundarySpan {
    start: usize,
    end: usize,
    multiline: bool,
    barrier_before: bool,
    barrier_after: bool,
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

#[instrument(skip(source), fields(bytes = source.len()))]
fn line_ending(source: &[u8]) -> Option<LineEnding> {
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
        None
    } else {
        Some(if has_crlf {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        })
    }
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
