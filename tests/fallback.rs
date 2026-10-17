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
    // The custom cop's class is registered, so Migration/DepartmentName qualifies `NoFoo` too.
    let ours = rbcop(&dir, true, &["-f", "e", "a.rb", "c.rb"]);
    let theirs = rubocop(&dir, &["--cache", "false", "-f", "e", "a.rb", "c.rb"]);
    assert_eq!(text(&ours.stderr), text(&theirs.stderr));
    assert_eq!(text(&ours.stdout), text(&theirs.stdout));
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

/// A plugin rbcop does not implement that defines no cops (rubocop-erb): rbcop honors its
/// default.yml (its `AllCops: Include` brings in the .erb files, its Lint/Syntax Exclude applies),
/// runs no cops on the files it brings in, names it on stderr, and under RBCOP_FALLBACK=1 hands
/// those files whole to RuboCop (#484).
#[test]
fn copless_plugin_files_go_to_rubocop() {
    let dir = project("copless");
    // Outside the project, so its lib/ is not inspected.
    let gems = dir.with_extension("gems");
    let _ = fs::remove_dir_all(&gems);
    let gem = gems.join("gems/rubocop-fakeerb-0.1.0");
    fs::create_dir_all(gems.join("specifications")).unwrap();
    fs::create_dir_all(gem.join("config")).unwrap();
    fs::create_dir_all(gem.join("lib/rubocop/fakeerb")).unwrap();
    fs::write(
        gems.join("specifications/rubocop-fakeerb-0.1.0.gemspec"),
        "Gem::Specification.new { |s| s.name = 'rubocop-fakeerb'; s.version = '0.1.0' }\n",
    )
    .unwrap();
    fs::write(
        gem.join("config/default.yml"),
        "inherit_mode:\n  merge:\n    - Exclude\n    - Include\n\nAllCops:\n  Include:\n    - '**/*.erb'\n\n\
         Lint/Syntax:\n  Exclude:\n    - '**/*.erb'\n    - 'skip/*.rb'\n",
    )
    .unwrap();
    fs::write(
        gem.join("lib/rubocop/fakeerb/plugin.rb"),
        "module RuboCop\n  module Fakeerb\n    class Plugin\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(dir.join(".rubocop.yml"), "plugins:\n  - rubocop-fakeerb\nAllCops:\n  NewCops: enable\n").unwrap();
    fs::remove_file(dir.join("a.rb")).unwrap();
    fs::remove_file(dir.join("no_foo.rb")).unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nx = 1\n").unwrap();
    fs::write(dir.join("v.erb"), "<p><%= foo(1,2) %></p>\nhello world\n").unwrap();
    fs::create_dir_all(dir.join("skip")).unwrap();
    fs::write(dir.join("skip/bad.rb"), "def (\n").unwrap();
    // A stand-in RuboCop: records how it was run and reports one offense on the last file.
    let child = dir.join("child.sh");
    fs::write(
        &child,
        "#!/bin/sh\necho \"$RBCOP_FALLBACK_SYNTAX $*\" >> \"$(dirname \"$0\")/child.log\"\nfor a; do f=$a; done\n\
         printf '%s\\t3\\t4\\tconvention\\t0\\t0\\t0\\tFake/Cop\\tFrom RuboCop.\\n' \"$f\"\n",
    )
    .unwrap();
    Command::new("chmod").arg("755").arg(&child).status().unwrap();
    let run = |fallback: bool| {
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env("GEM_PATH", &gems).env("RBCOP_RUBOCOP", &child);
        if fallback {
            cmd.env("RBCOP_FALLBACK", "1");
        }
        cmd.args(["-f", "e"]).current_dir(&dir).output().unwrap()
    };
    let out = run(false);
    // Without a ruby on PATH the gem cannot be found.
    if !text(&out.stderr).contains("cannot run ruby") {
        assert_eq!(
            text(&out.stderr),
            "rbcop: not running the Ruby code of plugin rubocop-fakeerb; set RBCOP_FALLBACK=1 to run them with \
             RuboCop\n"
        );
        // b.rb's offenses only: v.erb runs no cops, skip/bad.rb's Lint/Syntax is excluded.
        let stdout = text(&out.stdout);
        assert!(stdout.contains("b.rb:3:1"), "{stdout}");
        assert!(!stdout.contains("v.erb") && !stdout.contains("bad.rb"), "{stdout}");
        let out = run(true);
        assert_eq!(text(&out.stderr), "");
        let stdout = text(&out.stdout);
        assert!(stdout.contains("v.erb:1:4: C: Fake/Cop: From RuboCop."), "{stdout}");
        // One child, for v.erb alone, with every cop (no --except) and Lint/Syntax kept.
        let log = fs::read_to_string(dir.join("child.log")).unwrap();
        assert_eq!(log.lines().count(), 1, "{log}");
        assert!(log.starts_with("1 ") && !log.contains("--except") && log.trim_end().ends_with("v.erb"), "{log}");
    }
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&gems);
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
