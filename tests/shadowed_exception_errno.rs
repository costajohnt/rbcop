//! Lint/ShadowedException resolves rescued classes the way RuboCop's process does on the same
//! platform (#751). Expectations recorded from RuboCop 1.92.2 on x86_64-linux.

mod common;

use std::fs;

fn run(name: &str, src: &str) -> String {
    let dir = std::env::temp_dir().join(format!("rbcop-shadowed-errno-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("t.rb"), src).unwrap();
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--force-default-config", "--only", "Lint/ShadowedException", "-f", "emacs", "t.rb"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&dir);
    String::from_utf8(out.stdout).unwrap()
}

/// Linux: Errno::EOPNOTSUPP is Errno::ENOTSUP, so the first group sorts after the second; Errno::EADV
/// and Errno::EBADE are distinct classes (on macOS both are Errno::NOERROR and the answers swap).
#[cfg(not(target_os = "macos"))]
#[test]
fn errno_aliases_follow_the_platform() {
    let src = "begin\n  foo\nrescue Errno::ENOTSUP, IOError\n  bar\nrescue Errno::EOPNOTSUPP, EOFError\n  baz\nend\n\
               begin\n  foo\nrescue Errno::EADV, IOError\n  bar\nrescue Errno::EBADE, EOFError\n  baz\nend\n";
    let stdout = run("alias", src);
    let lines: Vec<&str> = stdout.lines().filter(|l| l.contains("ShadowedException")).collect();
    assert_eq!(lines.len(), 1, "{stdout}");
    assert!(lines[0].ends_with("t.rb:3:1: W: Lint/ShadowedException: Do not shadow rescued Exceptions."), "{stdout}");
}

/// RuboCop 1.92.2 no longer loads net/http, so Net::ReadTimeout is a NameError (nil) there.
#[test]
fn net_http_errors_are_unresolved() {
    let stdout = run("net", "begin\n  foo\nrescue Net::ReadTimeout, Timeout::Error\n  bar\nend\n");
    assert!(!stdout.contains("ShadowedException"), "{stdout}");
}
