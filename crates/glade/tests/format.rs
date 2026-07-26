use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn run_format(path: &PathBuf) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_glade"))
        .arg("format")
        .arg(path)
        .output()
        .expect("formatter runs")
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
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("{}\n", path.display())
    );
    fs::remove_file(path).expect("remove fixture");
}

#[test]
fn format_keeps_single_line_items_without_extra_blank_lines() {
    let path = fixture_path("canonical");
    let source = r"fn first() {}   fn second() {}
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
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("{}\n", path.display())
    );
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
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("{}\n", path.display())
    );
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
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("{}\n", path.display())
    );
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
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("{}\n", path.display())
    );
    fs::remove_file(path).expect("remove fixture");
}
