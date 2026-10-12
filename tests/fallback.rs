//! Cops rbcop cannot run: the stderr notice, and RBCOP_FALLBACK=1 (src/fallback.rs) compared with
//! a live RuboCop 1.91 when one is on PATH.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const COP: &str = r#"module RuboCop
  module Cop
    module Custom
      class NoFoo < Base
        extend AutoCorrector
        MSG = "Avoid `foo`.	Tab and \\ backslash."
        def on_send(node)
          return unless node.method?(:foo) && node.receiver.nil?
          add_offense(node.loc.selector) { |c| c.replace(node.loc.selector, 'bar') }
        end
      end
    end
  end
end
"#;

const CONFIG: &str = "require:\n  - ./no_foo.rb\nAllCops:\n  NewCops: enable\n  SuggestExtensions: false\n";

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbcop-fallback-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("no_foo.rb"), COP).unwrap();
    fs::write(dir.join(".rubocop.yml"), CONFIG).unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo(1)\nx = \"é\"; foo 2\nputs x\n").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo(3)\n").unwrap();
    dir
}

fn rbcop(dir: &Path, fallback: bool, args: &[&str]) -> Output {
    let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
    // RuboCop needs an existing HOME (CacheConfig.root_dir); the project has no user config.
    cmd.env("HOME", dir);
    if fallback {
        cmd.env("RBCOP_FALLBACK", "1");
    }
    cmd.args(args).current_dir(dir).output().unwrap()
}

fn rubocop(dir: &Path, args: &[&str]) -> Output {
    common::command("rubocop").env("HOME", dir).args(args).current_dir(dir).output().unwrap()
}

fn have_rubocop() -> bool {
    Command::new("rubocop").arg("-v").output().is_ok_and(|o| o.stdout == b"1.91.0\n")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn required_ruby_code_is_named_on_stderr() {
    let dir = project("notice");
    let out = rbcop(&dir, false, &["-f", "e"]);
    assert_eq!(
        text(&out.stderr),
        "rbcop: not running cops from Ruby code the config requires; set RBCOP_FALLBACK=1 to run them with RuboCop\n"
    );
    assert!(!text(&out.stdout).contains("Avoid"), "{}", text(&out.stdout));
    // Configured cops rbcop does not implement are counted by department.
    fs::write(dir.join(".rubocop.yml"), format!("{CONFIG}Custom/NoFoo:\n  Enabled: true\n")).unwrap();
    let out = rbcop(&dir, false, &["-f", "e"]);
    assert_eq!(
        text(&out.stderr),
        "rbcop: not running 1 cop it does not implement (Custom: 1) or cops from Ruby code the config requires; \
         set RBCOP_FALLBACK=1 to run them with RuboCop\n"
    );
    // No notice when --only leaves nothing rbcop cannot run... except the required code, which
    // may define any cop.
    let out = rbcop(&dir, false, &["-f", "e", "--only", "Style/StringLiterals"]);
    assert!(text(&out.stderr).contains("cops from Ruby code"), "{}", text(&out.stderr));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fallback_output_matches_rubocop() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
        return;
    }
    let dir = project("report");
    for format in ["simple", "emacs", "clang", "progress", "fuubar"] {
        let ours = rbcop(&dir, true, &["-f", format]);
        let theirs = rubocop(&dir, &["--cache", "false", "-f", format]);
        assert_eq!(text(&ours.stdout), text(&theirs.stdout), "{format}");
        assert_eq!(ours.status.code(), theirs.status.code(), "{format}");
        assert!(ours.stderr.is_empty(), "{}", text(&ours.stderr));
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fallback_autocorrect_matches_rubocop() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
        return;
    }
    let ours = project("ac-ours");
    let theirs = project("ac-theirs");
    let a = rbcop(&ours, true, &["-A", "-f", "e"]);
    let b = rubocop(&theirs, &["--cache", "false", "-A", "-f", "e"]);
    let rel = |s: String, d: &Path| s.replace(&d.display().to_string(), "DIR");
    assert_eq!(rel(text(&a.stdout), &ours), rel(text(&b.stdout), &theirs));
    for f in ["a.rb", "b.rb"] {
        assert_eq!(fs::read_to_string(ours.join(f)).unwrap(), fs::read_to_string(theirs.join(f)).unwrap(), "{f}");
    }
    let _ = fs::remove_dir_all(&ours);
    let _ = fs::remove_dir_all(&theirs);
}
