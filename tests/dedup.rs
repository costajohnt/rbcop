//! Runner#do_inspection_loop's `offenses_by_iteration.flatten.uniq`: offenses equal on line,
//! column, cop, message and severity collapse even when their ranges differ.

mod common;

use std::fs;

#[test]
fn offenses_at_one_position_collapse_regardless_of_range() {
    let dir = std::env::temp_dir().join(format!("rbcop-dedup-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    // MultipleComparison flags `a < b < c < d` and its inner `a < b < c` at the same column.
    fs::write(dir.join("t.rb"), "x = a < b < c < d\n").unwrap();
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--force-default-config", "--only", "Lint/MultipleComparison", "-f", "emacs", "t.rb"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}

/// The kept offense is the one RuboCop emits first: its pre-order reaches the parser `block` that
/// wraps `foo {}.to bar` before the receiver's block, so Lint/EmptyBlock keeps the outer range.
#[test]
fn first_offense_in_parser_order_is_kept() {
    let dir = std::env::temp_dir().join(format!("rbcop-dedup-order-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("t.rb"), "foo {}.to bar do\nend\n").unwrap();
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--force-default-config", "--only", "Lint/EmptyBlock", "-f", "json", "t.rb"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let want =
        r#""location":{"start_line":1,"start_column":1,"last_line":2,"last_column":3,"length":20,"line":1,"column":1}"#;
    assert!(stdout.contains(want), "{stdout}");
    assert_eq!(stdout.matches("Lint/EmptyBlock").count(), 1, "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}
