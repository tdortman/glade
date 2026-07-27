# Separate C++ and CUDA language backends

Status: accepted

C++ and CUDA use separate compiled language backends because CUDA extends C++ with its own Tree-sitter grammar and syntax, including kernel launches and CUDA qualifiers, while still accepting ordinary C++ constructs. Both backends emit the same source-preserving boundary model. The registry assigns `.cpp`, `.hpp`, `.cc`, `.cxx`, `.hxx`, and `.hh` to `backend-cpp`, assigns `.cu` and `.cuh` to `backend-cuda`, and keeps parser-specific classification outside `glade_core`; a shared private C-family helper is used only for identical traversal rules.

## Considered options

- Treat CUDA files as plain C++ and reject CUDA-only syntax.
- Select the C++ or CUDA parser inside one backend package.

Separate backends make extension ownership, diagnostics, parser assets, and future grammar differences explicit without weakening the common formatting contract.
