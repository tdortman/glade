use glade_core::{BackendResult, Boundary, Diagnostic, FormatPlan, LineEnding, Severity};
use std::ops::Range;
use tracing::{debug, instrument, trace, warn};
use tree_sitter::{Language, Node, Parser as TreeSitterParser};

#[instrument(skip(source, languages), fields(bytes = source.len(), backend = backend_id))]
/// Plans source formatting using one or more C-family Tree-sitter grammars.
///
/// Grammars are tried in order. This lets C++-family extensions use the CUDA
/// grammar when CUDA syntax is present while retaining the plain C++ grammar
/// as a compatibility fallback.
///
/// # Panics
///
/// Panics when a supplied grammar cannot be loaded by Tree-sitter.
pub fn plan_source(
    source: &[u8],
    languages: &[&Language],
    backend_id: &'static str,
    language_name: &'static str,
) -> BackendResult {
    let Some(line_ending) = line_ending(source) else {
        return BackendResult::Diagnostics(vec![diagnostic(
            backend_id,
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
            backend_id,
            format!("{language_name} source is not valid UTF-8"),
            None,
        )]);
    };

    let mut parser = TreeSitterParser::new();
    let mut selected_tree = None;
    let mut saw_incomplete_tree = false;
    let mut error_range = None;

    for language in languages {
        parser
            .set_language(language)
            .expect("C-family grammar loads");

        trace!("parsing source with tree-sitter");

        let tree = parser
            .parse(source_text, None)
            .expect("Tree-sitter returns a tree");

        let root = tree.root_node();

        if root.kind() != "translation_unit" || has_missing_node(root) {
            saw_incomplete_tree = true;
            continue;
        }

        if root.has_error() {
            error_range = first_error_node(root).map(|node| node.start_byte()..node.end_byte());
            continue;
        }

        selected_tree = Some(tree);
        break;
    }

    let Some(tree) = selected_tree else {
        warn!("all C-family grammars rejected the source");

        let message = if saw_incomplete_tree && error_range.is_none() {
            format!("{language_name} parser returned an incomplete syntax tree")
        } else {
            format!("{language_name} source contains syntax errors")
        };

        return BackendResult::Diagnostics(vec![diagnostic(backend_id, message, error_range)]);
    };

    let mut boundaries = Vec::new();
    format_container(source, tree.root_node(), line_ending, &mut boundaries);

    debug!(
        boundaries = boundaries.len(),
        "generated formatting boundaries"
    );

    BackendResult::Ready(FormatPlan {
        boundaries,
        diagnostics: Vec::new(),
    })
}

fn diagnostic(
    backend_id: &'static str,
    message: impl Into<String>,
    range: Option<Range<usize>>,
) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        range,
        message: message.into(),
        backend: backend_id,
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

    let eligible_indices: Vec<_> = children
        .iter()
        .enumerate()
        .filter_map(|(index, _child)| {
            is_primary_eligible_child(&children, index, container.kind()).then_some(index)
        })
        .collect();

    for child in &children {
        trace!(child_kind = child.kind(), "classified container child");
    }

    trace!(
        children = children.len(),
        eligible = eligible_indices.len(),
        separator = ?separator,
        "analysing container"
    );

    let boundary_spans: Vec<_> = eligible_indices
        .iter()
        .map(|index| {
            boundary_span(
                source,
                container,
                &children,
                *index,
                separator,
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

        let include_subgroup_barrier = previous_child.kind() == "preproc_include"
            && next_child.kind() == "preproc_include"
            && has_blank_line(&source[previous.end..next.start]);

        let force_after = is_pragma_once(source, previous_child)
            || (previous_child.kind() == "preproc_include"
                && next_child.kind() != "preproc_include");

        let range = previous.end..next.start;
        let required = previous.multiline || next.multiline || force_after;
        let barrier = previous.barrier_after || next.barrier_before || include_subgroup_barrier;

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
        format_nested_containers(source, child, line_ending, boundaries);
    }
}

fn format_nested_containers(
    source: &[u8],
    node: Node<'_>,
    line_ending: LineEnding,
    boundaries: &mut Vec<Boundary>,
) {
    if is_atomic(node.kind()) {
        trace!(kind = node.kind(), "skipping atomic node");
        return;
    }

    if is_container(node.kind()) {
        format_container(source, node, line_ending, boundaries);
        return;
    }

    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        format_nested_containers(source, child, line_ending, boundaries);
    }
}

fn is_atomic(kind: &str) -> bool {
    kind.starts_with("preproc_")
}

fn is_container(kind: &str) -> bool {
    matches!(
        kind,
        "translation_unit"
            | "declaration_list"
            | "field_declaration_list"
            | "enumerator_list"
            | "compound_statement"
            | "case_statement"
            | "labeled_statement"
    )
}

fn separator_for(kind: &str) -> Option<u8> {
    match kind {
        "translation_unit" | "declaration_list" => Some(b';'),
        "enumerator_list" => Some(b','),
        _ => None,
    }
}

fn is_eligible_child(container: &str, child: &str) -> bool {
    if is_atomic(child) {
        return true;
    }

    match container {
        "translation_unit" => is_declaration(child) || child == "expression_statement",
        "declaration_list" => is_declaration(child),
        "field_declaration_list" => is_member(child),
        "enumerator_list" => child == "enumerator",

        "compound_statement" | "case_statement" | "labeled_statement" => {
            is_statement_or_declaration(child)
        }

        _ => false,
    }
}

fn is_primary_eligible_child(children: &[Node<'_>], index: usize, container: &str) -> bool {
    is_eligible_child(container, children[index].kind())
}

fn is_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "alias_declaration"
            | "class_specifier"
            | "concept_definition"
            | "declaration"
            | "enum_specifier"
            | "function_definition"
            | "linkage_specification"
            | "namespace_alias_definition"
            | "namespace_definition"
            | "static_assert_declaration"
            | "struct_specifier"
            | "template_declaration"
            | "template_instantiation"
            | "type_definition"
            | "union_specifier"
            | "using_declaration"
    )
}

fn is_member(kind: &str) -> bool {
    is_declaration(kind)
        || matches!(
            kind,
            "field_declaration" | "function_definition" | "friend_declaration"
        )
}

fn is_statement_or_declaration(kind: &str) -> bool {
    is_declaration(kind)
        || kind.ends_with("_statement")
        || matches!(
            kind,
            "statement"
                | "for_range_loop"
                | "co_return_statement"
                | "template_declaration"
                | "type_definition"
                | "using_declaration"
                | "field_declaration"
        )
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
    container: Node<'_>,
    children: &[Node<'_>],
    index: usize,
    separator: Option<u8>,
    eligible: &[usize],
) -> BoundarySpan {
    let construct = children[index];

    let mut start = if container.kind() == "case_statement" && index == eligible[0] {
        container.start_byte()
    } else {
        construct.start_byte()
    };

    let mut previous = if container.kind() == "case_statement" && index == eligible[0] {
        0
    } else {
        index
    };

    while previous > 0 {
        let candidate = children[previous - 1];

        if !(is_attachment(candidate) || candidate.kind() == "access_specifier")
            || (has_blank_line(&source[candidate.end_byte()..start])
                && candidate.kind() != "attribute_specifier")
        {
            break;
        }

        if candidate.kind() == "comment"
            && has_preceding_eligible_on_line(source, children, previous - 1, eligible)
        {
            break;
        }

        start = candidate.start_byte();
        previous -= 1;
    }

    let mut end = boundary_end(source, construct);
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
        if candidate.kind() != "comment" || has_line_break(&source[end..candidate.start_byte()]) {
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
        && children[previous].kind() == "comment"
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

fn is_attachment(node: Node<'_>) -> bool {
    matches!(node.kind(), "comment" | "attribute_specifier")
}

fn is_pragma_once(source: &[u8], node: Node<'_>) -> bool {
    if node.kind() != "preproc_call" {
        return false;
    }

    let text = &source[node.start_byte()..node.end_byte()];

    let mut tokens = text
        .split(u8::is_ascii_whitespace)
        .filter(|token| !token.is_empty());

    match (tokens.next(), tokens.next(), tokens.next()) {
        (Some(b"#pragma"), Some(b"once"), None) => true,
        (Some(b"#"), Some(b"pragma"), Some(b"once")) => tokens.next().is_none(),
        _ => false,
    }
}

fn boundary_end(source: &[u8], node: Node<'_>) -> usize {
    if !is_atomic(node.kind()) {
        return node.end_byte();
    }

    let mut end = node.end_byte();

    if source.get(end.saturating_sub(1)) == Some(&b'\n') {
        end -= 1;

        if source.get(end.saturating_sub(1)) == Some(&b'\r') {
            end -= 1;
        }
    }

    end
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

        if comment.kind() != "comment"
            || has_line_break(&source[previous.end_byte()..comment.start_byte()])
        {
            return false;
        }

        if previous.kind() != "comment" {
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

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
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
