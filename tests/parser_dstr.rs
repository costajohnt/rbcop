//! ast::is_parser_dstr: a backslash-newline inside an interpolating literal is a line continuation,
//! so parser keeps a plain `str`; inside single quotes the newline stays and parser builds a `dstr`.

use std::fs;
use std::process::Command;

#[test]
fn backslash_newline_continuation_keeps_a_str() {
    let dir = std::env::temp_dir().join(format!("rbcop-parser-dstr-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    // Style/RedundantFileExtensionInRequire skips dstr arguments.
    fs::write(dir.join("t.rb"), "require \"foo\\\n.rb\"\nrequire 'bar\\\n.rb'\nrequire \"baz\\\\\n.rb\"\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--force-default-config", "--only", "Style/RedundantFileExtensionInRequire", "-f", "emacs", "t.rb"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "{stdout}");
    assert!(lines[0].contains("t.rb:2:1:"), "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}
