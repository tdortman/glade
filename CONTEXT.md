# Glade Formatter Context

This context defines the language for the syntax-aware whitespace formatter and its architecture effort.

## Language

**Boundary**:
A source location between adjacent eligible siblings. It requires canonical blank-line whitespace when either sibling's boundary span is multiline, except at container edges.
_Avoid_: Gap, separator

**Container**:
A syntax construct whose direct child statements, items, declarations, or members can be considered for boundary formatting.
_Avoid_: Scope, block (when referring to the general concept)

**Attachment**:
A comment or attribute macro owned by a neighboring construct for boundary calculations; contiguous leading attachments belong to the following construct, while trailing comments belong to the preceding construct.
_Avoid_: Comment handling

**Language backend**:
The language-specific component that parses source and emits generic formatting boundaries, attachments, and diagnostics for the core engine.
_Avoid_: Parser plugin, Tree-sitter adapter (unless specifically referring to an implementation)

**C++ family**:
C++ source files handled by the C++ language backend, covering `.cpp`, `.hpp`, `.cc`, `.cxx`, `.hxx`, and `.hh`.
_Avoid_: C-family files

**CUDA family**:
C++ source with CUDA extensions, handled by the CUDA language backend and covering `.cu` and `.cuh`.
_Avoid_: Independent language unrelated to C++

**Atomic construct**:
A macro invocation, macro definition, preprocessor directive subtree, or other construct whose internal token tree is treated as one indivisible source span by a language backend.
_Avoid_: Opaque node

**Boundary span**:
The source span used to decide whether a construct is multiline, including its owned attributes and comments while excluding whitespace at neighboring boundaries.
_Avoid_: Node span

**Editable whitespace range**:
A backend-selected source range containing only boundary whitespace. Preserved separators and attached comments remain outside this range.
_Avoid_: Gap range

**Barrier**:
A backend-marked boundary range that must remain unchanged because it contains a standalone comment or another non-editable separation.
_Avoid_: Skipped gap

**Backend registry**:
The compile-time mapping from file extensions to exactly one language backend in the CLI.
_Avoid_: Plugin registry

**Canonical normalization**:
The idempotent rule that emits exactly one blank line at a required boundary and no blank line where the boundary is not required.
_Avoid_: Whitespace cleanup

**Formatting drift**:
A source file whose canonical formatting computation would produce different
bytes; drift is distinct from an operational error.
_Avoid_: Formatting changes, stale formatting
