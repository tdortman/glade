# glade

Glade is a formatter that adds canonical blank lines between crowded, multiline language constructs. It changes boundary whitespace only. Comments, attributes, tokens, indentation, line endings, and all other source text stay under the caller's control.

## Supported languages

| Language | Extensions | Status    |
| -------- | ---------- | --------- |
| Rust     | `.rs`      | Supported |

Language support is selected by file extension. Unsupported extensions fail instead of being passed to the wrong formatter.

## Usage

Format files in place:

```console
glade format src/lib.rs src/main.rs
```

Check files without changing them:

```console
glade check src/**/*.rs
```

Shell globbing is intentional. Glade accepts explicit file paths and does not discover files from the working tree.

`check` exits with:

- `0` when every file is already canonical
- `1` when formatting drift is found
- `2` for usage, parsing, unsupported-language, read, or write errors

Diagnostics go to stderr. `check` diffs and success output go to stdout.

## Behaviour

Glade inserts one blank line between adjacent constructs when either boundary span is multiline. Boundary spans include owned outer attributes and attached comments. Standalone comment barriers, macro bodies, nested expressions, and container edges remain untouched.

Files with parser errors or mixed line endings are left byte-for-byte unchanged. LF and CRLF files retain their existing line-ending style. Formatting is idempotent, so running it twice produces the same bytes as running it once.

## Development

Run the strict lint and complete test suite:

```console
cargo clippy-strict
cargo nextest run --workspace
```

The workspace keeps the parser-neutral rewrite core, compiled backend registry, and language backends in separate crates. Adding a language requires a backend, registry entry, pinned parser assets, shared contract coverage, and language-specific fixtures.
