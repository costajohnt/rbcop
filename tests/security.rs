//! Hardening where rbcop deliberately differs from RuboCop.

mod common;

use std::fs;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rbcop-security-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    dir
}

/// A `.rubocop` arguments file must not be able to make rbcop write files, even when its `-o`
/// would only become complete by swallowing a path from the command line.
#[test]
fn rubocop_arguments_file_cannot_choose_output_files() {
    for args_file in ["-o\n", "--format json --out\n", "--ou=x.txt\n", "-Do\n"] {
        let dir = scratch("argsfile");
        fs::write(dir.join(".rubocop"), args_file).unwrap();
        fs::write(dir.join("app.rb"), "puts 1\n").unwrap();
        let out = common::command(env!("CARGO_BIN_EXE_rbcop")).arg("app.rb").current_dir(&dir).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args_file:?}");
        assert_eq!(fs::read_to_string(dir.join("app.rb")).unwrap(), "puts 1\n", "{args_file:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}

/// HTML output escapes `<` in messages outside code spans, and paths.
#[test]
fn html_escapes_messages_and_paths() {
    let dir = scratch("html");
    let name = "x<b>.rb";
    fs::write(dir.join(name), "binding.pry\n").unwrap();
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--force-default-config", "--only", "Lint/Debugger", "-f", "html", name])
        .current_dir(&dir)
        .output()
        .unwrap();
    let html = String::from_utf8(out.stdout).unwrap();
    assert!(!html.contains("x<b>"), "unescaped path in HTML output");
    assert!(html.contains("x&lt;b&gt;.rb"));
    let _ = fs::remove_dir_all(&dir);
}

/// -a/-A writes each corrected file after the whole run is inspected; a file that changed on disk
/// since it was read and corrected (an editor save mid-run) is left alone, not overwritten with the
/// correction of its old content (#722).
#[test]
fn autocorrect_write_skips_a_file_changed_since_read() {
    let dir = scratch("changed-since-read");
    let file = dir.join("a.rb");
    fs::write(&file, "edited\n").unwrap();
    let roots = [fs::canonicalize(&dir).unwrap()];
    // Same length, other bytes; then another length.
    for read in [&b"before\n"[..], b"x\n"] {
        assert!(!rbcop::autocorrect::write_source(&file, "corrected\n", Some(read), &roots).unwrap());
        assert_eq!(fs::read_to_string(&file).unwrap(), "edited\n");
    }
    assert!(rbcop::autocorrect::write_source(&file, "corrected\n", Some(b"edited\n"), &roots).unwrap());
    assert_eq!(fs::read_to_string(&file).unwrap(), "corrected\n");
    fs::remove_dir_all(&dir).unwrap();
}
