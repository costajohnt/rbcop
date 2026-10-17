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
    // Required code may define any cop, so --only does not silence it.
    let out = rbcop(&dir, false, &["-f", "e", "--only", "Style/StringLiterals"]);
    assert!(text(&out.stderr).contains("cops from Ruby code"), "{}", text(&out.stderr));
    // A plugin's cops are counted by department: a fake plugin gem found through GEM_PATH.
    let gems = dir.join("gems");
    fs::create_dir_all(gems.join("specifications")).unwrap();
    fs::create_dir_all(gems.join("gems/rubocop-fake-0.1.0/config")).unwrap();
    fs::write(
        gems.join("specifications/rubocop-fake-0.1.0.gemspec"),
        "Gem::Specification.new { |s| s.name = 'rubocop-fake'; s.version = '0.1.0' }\n",
    )
    .unwrap();
    fs::write(
        gems.join("gems/rubocop-fake-0.1.0/config/default.yml"),
        "Fake/One:\n  Enabled: true\nFake/Two:\n  Enabled: true\nFake/Off:\n  Enabled: false\nFake/Sub:\n  Enabled: \
         false\nFake/Sub/Three:\n  Enabled: true\n",
    )
    .unwrap();
    // A nested department (rubocop-i18n's I18n/GetText) is not a cop, enabled or not (#423).
    fs::write(
        dir.join(".rubocop.yml"),
        "plugins:\n  - rubocop-fake\nAllCops:\n  NewCops: enable\nFake/Sub:\n  Enabled: true\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env("GEM_PATH", &gems).args(args).current_dir(&dir).output().unwrap()
    };
    let out = run(&["-f", "e", "a.rb"]);
    // Without a ruby on PATH the gem cannot be found.
    if !text(&out.stderr).contains("cannot run ruby") {
        assert_eq!(
            text(&out.stderr),
            "rbcop: not running 3 cops it does not implement (Fake: 2, Fake/Sub: 1); set RBCOP_FALLBACK=1 to run \
             them with RuboCop\n"
        );
        // ...and --only/--except leave out what they deselect.
        let out = run(&["-f", "e", "--except", "Fake/One", "a.rb"]);
        assert!(text(&out.stderr).contains("(Fake: 1, Fake/Sub: 1)"), "{}", text(&out.stderr));
        let out = run(&["-f", "e", "--only", "Style/StringLiterals", "a.rb"]);
        assert_eq!(text(&out.stderr), "");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// rbcop under RBCOP_FALLBACK=1 and RuboCop print the same stdout and stderr and exit alike.
fn same(dir: &Path, env: &[(&str, &str)], args: &[&str]) {
    let mut ours = common::command(env!("CARGO_BIN_EXE_rbcop"));
    ours.env("HOME", dir).env("RBCOP_FALLBACK", "1");
    let mut theirs = common::command("rubocop");
    theirs.env("HOME", dir).args(["--cache", "false"]);
    for (k, v) in env {
        ours.env(k, v);
        theirs.env(k, v);
    }
    let ours = ours.args(args).current_dir(dir).output().unwrap();
    let theirs = theirs.args(args).current_dir(dir).output().unwrap();
    assert_eq!(text(&ours.stdout), text(&theirs.stdout), "{env:?} {args:?}");
    assert_eq!(text(&ours.stderr), text(&theirs.stderr), "{env:?} {args:?}");
    assert_eq!(ours.status.code(), theirs.status.code(), "{env:?} {args:?}");
}

#[test]
fn fallback_forwards_the_command_line() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
        return;
    }
    let dir = project("flags");
    fs::write(dir.join("c.rb"), "# frozen_string_literal: true\n\nfoo(1) # rubocop:disable Custom/NoFoo\n").unwrap();
    // A pending custom cop runs under `NewCops: enable`, of AllCops or of its department (#413).
    let dept_new_cops = "require:\n  - ./no_foo.rb\nAllCops:\n  SuggestExtensions: false\nCustom:\n  NewCops: enable\n";
    for config in [CONFIG, dept_new_cops] {
        fs::write(dir.join(".rubocop.yml"), format!("{config}Custom/NoFoo:\n  Enabled: pending\n")).unwrap();
        for flags in [&[][..], &["--disable-pending-cops"], &["--enable-pending-cops"]] {
            same(&dir, &[], &[&["-f", "e", "b.rb"], flags].concat());
        }
    }
    // Without NewCops the pending-cops banner is printed once.
    fs::write(dir.join(".rubocop.yml"), "require:\n  - ./no_foo.rb\nCustom/NoFoo:\n  Enabled: pending\n").unwrap();
    same(&dir, &[], &["-f", "e", "b.rb"]);
    // #415, and --editor-mode (`AutoCorrect: contextual` is not correctable under it).
    fs::write(dir.join(".rubocop.yml"), format!("{CONFIG}Custom/NoFoo:\n  AutoCorrect: contextual\n")).unwrap();
    for flag in ["--ignore-disable-comments", "--editor-mode"] {
        same(&dir, &[], &["-f", "e", flag, "b.rb", "c.rb"]);
    }
    // The child reads neither `.rubocop` nor RUBOCOP_OPTS a second time (#414).
    for opts in ["--format progress", "--format json", "--stderr", "-f s --stderr"] {
        fs::write(dir.join(".rubocop"), format!("{opts}\n")).unwrap();
        same(&dir, &[], &["a.rb"]);
        fs::remove_file(dir.join(".rubocop")).unwrap();
        same(&dir, &[("RUBOCOP_OPTS", opts)], &["a.rb"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fallback_prints_config_warnings_once() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
        return;
    }
    // #416: a config warning, and the missing-department warnings for directive names (of a cop
    // rbcop implements, and of a custom one: the child warns about that one as its offense on the
    // line consults the directive).
    let dir = project("warnings");
    fs::write(dir.join(".rubocop.yml"), format!("{CONFIG}Style/StringLiterals:\n  Foo: 1\n")).unwrap();
    fs::write(dir.join("c.rb"), "# frozen_string_literal: true\n\nfoo(1) # rubocop:disable NoFoo, LineLength\n")
        .unwrap();
    // Migration/DepartmentName's stdout differs (rbcop cannot qualify a custom cop's name).
    let ours = rbcop(&dir, true, &["-f", "e", "a.rb", "c.rb"]);
    let theirs = rubocop(&dir, &["--cache", "false", "-f", "e", "a.rb", "c.rb"]);
    assert_eq!(text(&ours.stderr), text(&theirs.stderr));
    assert!(text(&theirs.stderr).contains("no department given for NoFoo"), "{}", text(&theirs.stderr));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fallback_fail_fast_autocorrect_stops_correcting() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
        return;
    }
    // -A -F: an uncorrectable offense in a.rb stops the run, so the fallback corrects no b.rb.
    let ours = project("ff-ours");
    let theirs = project("ff-theirs");
    for d in [&ours, &theirs] {
        fs::write(d.join("a.rb"), "# frozen_string_literal: true\n\neval('1')\n").unwrap();
    }
    let a = rbcop(&ours, true, &["-A", "-F", "-f", "e", "a.rb", "b.rb"]);
    let b = rubocop(&theirs, &["--cache", "false", "-A", "-F", "-f", "e", "a.rb", "b.rb"]);
    let rel = |s: String, d: &Path| s.replace(&d.display().to_string(), "DIR");
    assert_eq!(rel(text(&a.stdout), &ours), rel(text(&b.stdout), &theirs));
    assert_eq!(a.status.code(), b.status.code());
    for f in ["a.rb", "b.rb"] {
        assert_eq!(fs::read_to_string(ours.join(f)).unwrap(), fs::read_to_string(theirs.join(f)).unwrap(), "{f}");
    }
    let _ = fs::remove_dir_all(&ours);
    let _ = fs::remove_dir_all(&theirs);
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
