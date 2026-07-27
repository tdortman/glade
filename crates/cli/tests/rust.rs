use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn run_format(path: &std::path::Path) -> std::process::Output {
    run_cli("format", &[path])
}

fn run_cli(command: &str, paths: &[&std::path::Path]) -> std::process::Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_glade"));
    process.arg(command);

    for path in paths {
        process.arg(path);
    }

    process.output().expect("formatter runs")
}

fn fixture_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("glade-{name}-{}-{nonce}.rs", std::process::id()))
}

#[test]
fn format_adds_one_blank_line_between_multiline_top_level_items() {
    let path = fixture_path("top-level");

    fs::write(
        &path,
        r"struct First {
    value: i32,
}
fn second() {}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"struct First {
    value: i32,
}

fn second() {}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_keeps_single_line_items_without_extra_blank_lines() {
    let path = fixture_path("canonical");

    let source = r"fn first() {}   fn second() {}
";

    fs::write(&path, source).expect("write fixture");

    #[cfg(unix)]
    let before_inode = {
        use std::os::unix::fs::MetadataExt;

        fs::metadata(&path).expect("read fixture metadata").ino()
    };

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(fs::read_to_string(&path).expect("read fixture"), source);

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        assert_eq!(
            fs::metadata(&path).expect("read formatted metadata").ino(),
            before_inode
        );
    }

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_preserves_comment_barriers_with_indented_blank_lines() {
    let path = fixture_path("comment-barrier");

    let source = r"// standalone

fn first() {}
fn second() {}
";

    fs::write(&path, source).expect("write fixture");
    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(fs::read_to_string(&path).expect("read fixture"), source);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_preserves_same_line_trailing_comments() {
    let path = fixture_path("trailing-comment");

    let source = r"fn first() {} /* one */ /* two */
fn second() {}
";

    fs::write(&path, source).expect("write fixture");
    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(fs::read_to_string(&path).expect("read fixture"), source);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_keeps_multiline_trailing_comments_with_their_item() {
    let path = fixture_path("multiline-trailing-comment");

    fs::write(
        &path,
        r"fn first() {} /* one
 two */
fn second() {}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"fn first() {} /* one
 two */

fn second() {}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_preserves_standalone_comment_barriers() {
    let path = fixture_path("standalone-comment");

    let source = r"fn first() {}


// standalone
fn second() {}
";

    fs::write(&path, source).expect("write fixture");
    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(fs::read_to_string(&path).expect("read fixture"), source);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_includes_outer_attributes_in_boundary_spans() {
    let path = fixture_path("outer-attribute");

    fs::write(
        &path,
        r"fn zero() {}
#[cfg(any(
    unix,
))]

fn first() {}
fn second() {}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"fn zero() {}

#[cfg(any(
    unix,
))]

fn first() {}

fn second() {}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_keeps_inner_attributes_owned_by_the_container() {
    let path = fixture_path("inner-attribute");

    let source = r"#![allow(
    dead_code,
)]
fn first() {}   fn second() {}
";

    fs::write(&path, source).expect("write fixture");
    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        source
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_attaches_contiguous_leading_comments() {
    let path = fixture_path("leading-comments");

    fs::write(
        &path,
        r"fn first() {}
// lead one
// lead two
fn second() {}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"fn first() {}

// lead one
// lead two
fn second() {}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_preserves_blank_line_barriers_after_trailing_comments() {
    let path = fixture_path("trailing-comment-barrier");

    let source = r"fn first() {} // trailing

fn second() {}
";

    fs::write(&path, source).expect("write fixture");
    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        source
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_separates_items_inside_inline_modules() {
    let path = fixture_path("module");

    fs::write(
        &path,
        r"mod inner {
    struct First {
        value: i32,
    }
    fn second() {}
}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"mod inner {
    struct First {
        value: i32,
    }

    fn second() {}
}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_separates_block_and_closure_children() {
    let path = fixture_path("blocks");

    fs::write(
        &path,
        r"fn blocks() {
    let first = [
        1,
        2,
    ]; // trailing
    consume(|| {
        let nested_first = [
            1,
            2,
        ];
        let nested_second = 2;
    });
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
    &source[start..end] // trailing
}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"fn blocks() {
    let first = [
        1,
        2,
    ]; // trailing

    consume(|| {
        let nested_first = [
            1,
            2,
        ];

        let nested_second = 2;
    });
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

    &source[start..end] // trailing
}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_separates_impl_and_trait_items() {
    let path = fixture_path("associated-items");

    fs::write(
        &path,
        r"struct Service;
impl Service {
    methods!(
        first,
    );
    fn first(&self) {}
    fn second(&self) {}
}
trait Handler {
    type Output = (
        i32,
    );
    fn first(&self) {
        let value = 1;
    }
    fn second(&self) {}
}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"struct Service;

impl Service {
    methods!(
        first,
    );

    fn first(&self) {}
    fn second(&self) {}
}

trait Handler {
    type Output = (
        i32,
    );

    fn first(&self) {
        let value = 1;
    }

    fn second(&self) {}
}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_separates_match_arms_and_members() {
    let path = fixture_path("members");

    fs::write(
        &path,
        r"fn choose(value: bool) {
    match value {
        true => {
            let first = [
                1,
                2,
            ];
            let second = 2;
        }
        false => {}
    }
    match value {
        true => call(
            value,
        ),
        false => {}
    }
}
struct Record {
    first: (
        i32,
    ),
    second: i32,
}
struct Tuple(
    #[cfg(any(
        unix,
    ))]
    pub i32,
    i64,
);

enum Choice {
    First {
        value: (
            i32,
        ) /* trailing */,
    },
    Second,
}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"fn choose(value: bool) {
    match value {
        true => {
            let first = [
                1,
                2,
            ];
            let second = 2;
        }

        false => {}
    }

    match value {
        true => call(
            value,
        ),

        false => {}
    }
}

struct Record {
    first: (
        i32,
    ),

    second: i32,
}

struct Tuple(
    #[cfg(any(
        unix,
    ))]
    pub i32,

    i64,
);

enum Choice {
    First {
        value: (
            i32,
        ) /* trailing */,
    },

    Second,
}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_does_not_traverse_nested_expressions_or_macros() {
    let path = fixture_path("atomic");

    let source = r"macro_rules! unchanged {
    () => {{
        let first = [
            1,
            2,
        ];
        let second = 2;
    }};
}
fn expressions() {
    let nested = {
        let first = [
            1,
            2,
        ];
        let second = 2;
    };
    let conditional = if value {
        let first = [
            1,
            2,
        ];
        let second = 2;
    } else {
        0
    };

    unchanged! {
        let first = [
            1,
            2,
        ];
        let second = 2;
    }
    let values = [
        1,
        2,
    ];
}
";

    fs::write(&path, source).expect("write fixture");
    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"macro_rules! unchanged {
    () => {{
        let first = [
            1,
            2,
        ];
        let second = 2;
    }};
}

fn expressions() {
    let nested = {
        let first = [
            1,
            2,
        ];
        let second = 2;
    };

    let conditional = if value {
        let first = [
            1,
            2,
        ];
        let second = 2;
    } else {
        0
    };

    unchanged! {
        let first = [
            1,
            2,
        ];
        let second = 2;
    }

    let values = [
        1,
        2,
    ];
}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_preserves_items_inside_nested_blocks() {
    let path = fixture_path("nested-struct");

    let source = r"fn expression() {
    let nested = {
        struct Inner {
            first: (
                i32,
            ),
            second: i32,
        }
        let first = [
            1,
            2,
        ];
        let second = 2;
    };
}
";

    fs::write(&path, source).expect("write fixture");
    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);

    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        source
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_preserves_unicode_bytes() {
    let path = fixture_path("unicode");

    fs::write(
        &path,
        r"struct Café {
    valeur: i32,
}
fn second() {}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    assert_eq!(
        fs::read(&path).expect("read formatted fixture"),
        r"struct Café {
    valeur: i32,
}

fn second() {}
"
        .as_bytes(),
    );

    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_preserves_lf_line_endings() {
    let path = fixture_path("lf");

    fs::write(
        &path,
        r"struct First {
    value: i32,
}
fn second() {}
",
    )
    .expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    assert_eq!(
        fs::read(&path).expect("read formatted fixture"),
        r"struct First {
    value: i32,
}

fn second() {}
"
        .as_bytes(),
    );

    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_preserves_crlf_line_endings() {
    let path = fixture_path("crlf");

    let source = r"struct First {
    value: i32,
}
fn second() {}
";

    let source = source.replace('\n', "\r\n");
    fs::write(&path, source.as_bytes()).expect("write fixture");
    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let expected = r"struct First {
    value: i32,
}

fn second() {}
"
    .replace('\n', "\r\n");

    assert_eq!(
        fs::read(&path).expect("read formatted fixture"),
        expected.as_bytes()
    );

    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_rejects_mixed_line_endings_without_rewriting() {
    let path = fixture_path("mixed-line-endings");

    let source = r"struct First {
    value: i32,
}
fn second() {}
";

    let source = source.replace('\n', "\r\n");
    let source = source.trim_end_matches("\r\n").to_owned() + "\n";
    fs::write(&path, source.as_bytes()).expect("write fixture");
    let output = run_format(&path);
    assert!(!output.status.success());
    assert_eq!(fs::read(&path).expect("read fixture"), source.as_bytes());
    assert!(String::from_utf8_lossy(&output.stderr).contains("mixes LF and CRLF"));
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_rejects_malformed_source_without_rewriting() {
    let path = fixture_path("malformed");

    let source = r"fn broken(
";

    fs::write(&path, source).expect("write fixture");
    let output = run_format(&path);
    assert!(!output.status.success());
    assert_eq!(fs::read(&path).expect("read fixture"), source.as_bytes());
    assert!(String::from_utf8_lossy(&output.stderr).contains("syntax errors"));
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_rejects_invalid_utf8_without_rewriting() {
    let path = fixture_path("invalid-utf8");

    let mut source = br"fn first() {}
"
    .to_vec();

    source.push(0xFF);
    fs::write(&path, &source).expect("write fixture");
    let output = run_format(&path);
    assert!(!output.status.success());
    assert_eq!(fs::read(&path).expect("read fixture"), source);
    assert!(String::from_utf8_lossy(&output.stderr).contains("not valid UTF-8"));
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_rejects_missing_structure_without_rewriting() {
    let path = fixture_path("missing-structure");

    let source = r"fn broken() {
    let value = (1;
}
";

    fs::write(&path, source).expect("write fixture");
    let output = run_format(&path);
    assert!(!output.status.success());
    assert_eq!(fs::read(&path).expect("read fixture"), source.as_bytes());
    assert!(String::from_utf8_lossy(&output.stderr).contains("incomplete syntax tree"));
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_is_idempotent() {
    let path = fixture_path("idempotent");

    fs::write(
        &path,
        r"struct First {
    value: i32,
}
fn second() {}
",
    )
    .expect("write fixture");

    let first = run_format(&path);
    assert!(first.status.success(), "stderr: {:?}", first.stderr);
    assert!(first.stdout.is_empty());
    assert!(first.stderr.is_empty());
    let formatted = fs::read(&path).expect("read formatted fixture");
    let second = run_format(&path);
    assert!(second.status.success(), "stderr: {:?}", second.stderr);
    assert_eq!(fs::read(&path).expect("read fixture"), formatted);
    assert!(second.stdout.is_empty());
    assert!(second.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn verbose_flag_emits_tracing_details() {
    let path = fixture_path("verbose");

    fs::write(
        &path,
        r"struct First {
    value: i32,
}
fn second() {}
",
    )
    .expect("write fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_glade"))
        .arg("-vvv")
        .arg("format")
        .arg(&path)
        .output()
        .expect("formatter runs");

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("formatting input files"),
        "binary={} stderr={stderr}",
        env!("CARGO_BIN_EXE_glade")
    );

    assert!(
        stderr.contains("parsing source with tree-sitter"),
        "{stderr}"
    );

    assert!(stderr.contains("applying formatting patch"), "{stderr}");
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn check_reports_drift_without_mutating_the_file() {
    let path = fixture_path("check-drift");

    let source = r"struct First {
    value: i32,
}
fn second() {}
";

    fs::write(&path, source).expect("write fixture");
    let output = run_cli("check", &[path.as_path()]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&path).expect("read fixture"), source);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let diff_path = path.display().to_string().replace('\\', "/");
    assert!(stdout.contains(&format!("--- {diff_path}")));
    assert!(stdout.contains(&format!("+++ {diff_path}")));
    assert!(stdout.contains("@@"));
    assert!(stdout.contains("\n+\n"));
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn check_reports_success_for_canonical_input() {
    let path = fixture_path("check-canonical");
    let source = "fn first() {}   fn second() {}\n";
    fs::write(&path, source).expect("write fixture");
    let output = run_cli("check", &[path.as_path()]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&path).expect("read fixture"), source);
    assert_eq!(output.stdout, b"All checks passed!\n");
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn check_reports_operational_errors_with_status_two() {
    let path = fixture_path("check-error");
    let source = b"fn broken(";
    fs::write(&path, source).expect("write fixture");
    let output = run_cli("check", &[path.as_path()]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&path).expect("read fixture"), source);
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("syntax errors"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("line"));
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_continues_after_one_file_fails() {
    let bad_path = fixture_path("multi-error").with_extension("txt");
    let parse_path = fixture_path("multi-parse");
    let good_path = fixture_path("multi-good");

    let source = r"struct First {
    value: i32,
}
fn second() {}
";

    fs::write(&bad_path, "not Rust").expect("write bad fixture");
    fs::write(&parse_path, "fn broken(").expect("write parse fixture");
    fs::write(&good_path, source).expect("write good fixture");

    let output = run_cli("format", &[
        bad_path.as_path(),
        parse_path.as_path(),
        good_path.as_path(),
    ]);

    assert_eq!(output.status.code(), Some(2));

    assert_eq!(
        fs::read_to_string(&good_path).expect("read good fixture"),
        r"struct First {
    value: i32,
}

fn second() {}
"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&bad_path.display().to_string()));
    assert!(stderr.contains("unsupported file extension"));
    assert!(stderr.contains(&parse_path.display().to_string()));
    assert!(stderr.contains("syntax errors"));
    assert!(output.stdout.is_empty());
    fs::remove_file(bad_path).expect("remove bad fixture");
    fs::remove_file(parse_path).expect("remove parse fixture");
    fs::remove_file(good_path).expect("remove good fixture");
}

#[test]
fn format_preserves_permissions_when_replacing_content() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let path = fixture_path("permissions");

        fs::write(
            &path,
            r"struct First {
    value: i32,
}
fn second() {}
",
        )
        .expect("write fixture");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))
            .expect("set fixture permissions");

        let before = fs::metadata(&path).expect("read fixture metadata");
        let output = run_cli("format", &[path.as_path()]);
        let after = fs::metadata(&path).expect("read formatted metadata");

        assert!(output.status.success(), "stderr: {:?}", output.stderr);
        assert_eq!(after.permissions().mode(), before.permissions().mode());
        assert_ne!(after.ino(), before.ino());

        fs::remove_file(path).expect("remove fixture");
    }
}

#[test]
fn missing_files_are_usage_errors() {
    let output = run_cli("check", &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("required"));
}

#[test]
fn format_preserves_use_subgroups_and_separates_following_items() {
    let path = fixture_path("use-subgroups");
    let source = r"use std::fmt;

use std::io;
fn main() {}
";

    fs::write(&path, source).expect("write fixture");

    let output = run_format(&path);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(
        fs::read_to_string(&path).expect("read formatted fixture"),
        r"use std::fmt;

use std::io;

fn main() {}
"
    );

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_file(path).expect("remove fixture");
}
