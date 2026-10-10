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
