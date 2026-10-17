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
        "Fake/One:\n  Enabled: true\nFake/Two:\n  Enabled: true\nFake/Off:\n  Enabled: false\n",
    )
    .unwrap();
    fs::write(dir.join(".rubocop.yml"), "plugins:\n  - rubocop-fake\nAllCops:\n  NewCops: enable\n").unwrap();
    let run = |args: &[&str]| {
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env("GEM_PATH", &gems).args(args).current_dir(&dir).output().unwrap()
    };
    let out = run(&["-f", "e", "a.rb"]);
    // Without a ruby on PATH the gem cannot be found.
    if !text(&out.stderr).contains("cannot run ruby") {
        assert_eq!(
            text(&out.stderr),
            "rbcop: not running 2 cops it does not implement (Fake: 2); set RBCOP_FALLBACK=1 to run them with \
             RuboCop\n"
        );
        // ...and --only/--except leave out what they deselect.
        let out = run(&["-f", "e", "--except", "Fake/One", "a.rb"]);
        assert!(text(&out.stderr).contains("(Fake: 1)"), "{}", text(&out.stderr));
        let out = run(&["-f", "e", "--only", "Style/StringLiterals", "a.rb"]);
        assert_eq!(text(&out.stderr), "");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// RuboCop's registry holds a plugin's cop classes: one without a default.yml entry is a cop
/// (enabled), a default.yml entry without a class is not (an unknown name), and --show-cops lists
/// them with `# Supports --autocorrect` for the cops that extend AutoCorrector.
#[test]
fn plugin_cops_come_from_lib_classes() {
    let dir = project("lib-classes");
    let gems = dir.join("gems");
    let gem = gems.join("gems/rubocop-fake-0.1.0");
    fs::create_dir_all(gems.join("specifications")).unwrap();
    fs::create_dir_all(gem.join("config")).unwrap();
    fs::create_dir_all(gem.join("lib/rubocop/cop/fake")).unwrap();
    fs::write(
        gems.join("specifications/rubocop-fake-0.1.0.gemspec"),
        "Gem::Specification.new { |s| s.name = 'rubocop-fake'; s.version = '0.1.0' }\n",
    )
    .unwrap();
    fs::write(gem.join("config/default.yml"), "Fake/One:\n  Enabled: true\nFake/Phantom:\n  Enabled: true\n").unwrap();
    fs::write(
        gem.join("lib/rubocop/cop/fake/base.rb"),
        "module RuboCop # :nodoc:\n  module Cop\n    module Fake\n      class Base < ::RuboCop::Cop::Base\n        \
         exclude_from_registry\n        extend AutoCorrector\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(
        gem.join("lib/rubocop/cop/fake/one.rb"),
        "module RuboCop\n  module Cop\n    module Fake\n      class One < Base # :nodoc:\n        def on_send(node)\n        \
         end\n      end\n\n      class Helper < Struct.new(:a); end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(
        gem.join("lib/rubocop/cop/fake/extra.rb"),
        "module RuboCop::Cop::Fake\n  class Extra < RuboCop::Cop::Base\n  end\nend\n",
    )
    .unwrap();
    fs::write(dir.join(".rubocop.yml"), "plugins:\n  - rubocop-fake\nAllCops:\n  NewCops: enable\n").unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nputs 1 # rubocop:disable Fake/Phantom\n").unwrap();
    let run = |args: &[&str]| {
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env("GEM_PATH", &gems).args(args).current_dir(&dir).output().unwrap()
    };
    let out = run(&["-f", "e", "a.rb"]);
    // Without a ruby on PATH the gem cannot be found.
    if !text(&out.stderr).contains("cannot run ruby") {
        assert!(
            text(&out.stderr).contains("not running 2 cops it does not implement (Fake: 2)"),
            "{}",
            text(&out.stderr)
        );
        assert!(text(&out.stdout).contains("Unknown cop name `Fake/Phantom`"), "{}", text(&out.stdout));
        let out = run(&["--show-cops", "Fake/*"]);
        assert_eq!(
            text(&out.stdout),
            "Fake/Extra:\n  Enabled: true\n\n# Supports --autocorrect\nFake/One:\n  Enabled: true\n\n"
        );
    }
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
