//! rbcop output on tests/fixtures must equal RuboCop 1.88.2's (recorded in expected.txt).

use std::process::Command;

#[test]
fn matches_rubocop_on_fixtures() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let out = Command::new(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--force-default-config", "--format", "emacs"])
        .arg(dir)
        .output()
        .unwrap();
    let actual: String = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| format!("{}\n", l.rsplit_once("/fixtures/").unwrap().1))
        .collect();
    let expected = std::fs::read_to_string(format!("{dir}/expected.txt")).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(out.status.code(), Some(1));
}
