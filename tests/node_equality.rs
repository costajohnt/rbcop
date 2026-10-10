//! Structural node equality (helpers::node_equality) must include heredoc bodies, which lie
//! outside a node's source range: same opener text with different bodies is not equal.

use std::fs;
use std::process::Command;

#[test]
fn heredoc_bodies_take_part_in_node_equality() {
    let dir = std::env::temp_dir().join(format!("rbcop-node-equality-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    let src = "x = foo(<<~X) == foo(<<~X)\n  a\nX\n  b\nX\ny = foo(<<~X) == foo(<<~X)\n  a\nX\n  a\nX\n";
    fs::write(dir.join("t.rb"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--force-default-config", "--only", "Lint/BinaryOperatorWithIdenticalOperands", "-f", "emacs", "t.rb"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "{stdout}");
    assert!(lines[0].contains("t.rb:6:5:"), "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}
