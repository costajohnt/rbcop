//! `--no-ruby` / `RBCOP_NO_RUBY=1` (#771): rbcop starts no `ruby`, `rubocop` or `bundle`, even
//! under RBCOP_FALLBACK=1. What needs Ruby fails with exit 2; the cops the RuboCop fallback would
//! run are skipped, named on stderr and listed in `-f json`.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

const COP: &str = "module RuboCop\n  module Cop\n    module Custom\n      class NoFoo < Base\n      end\n    end\n  \
                   end\nend\n";

/// A project whose PATH holds only stand-in `ruby`, `rubocop` and `bundle`, each leaving a
/// `ran-<name>` file when started, with a bundle that locks rubocop (so the fallback picks
/// `bundle exec rubocop`).
fn project(name: &str, config: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("rbcop-no-ruby-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("bin")).unwrap();
    for exe in ["ruby", "rubocop", "bundle"] {
        let path = dir.join("bin").join(exe);
        fs::write(&path, format!("#!/bin/sh\n: > \"${{0%/*}}/../ran-{exe}\"\nexit 1\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("Gemfile.lock"), "GEM\n  specs:\n    rubocop (1.92.2)\n").unwrap();
    fs::write(dir.join("no_foo.rb"), COP).unwrap();
    fs::write(dir.join(".rubocop.yml"), config).unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nputs 1\n").unwrap();
    dir
}

fn rbcop(dir: &Path, env: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
    cmd.env("HOME", dir).env("PATH", dir.join("bin")).env("RBCOP_FALLBACK", "1");
    // No pending-cops banner on stderr.
    cmd.arg("--disable-pending-cops");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.args(args).current_dir(dir).output().unwrap()
}

/// The stand-ins started since the last call (their markers are removed).
fn ran(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.unwrap().file_name().into_string().ok())
        .filter(|n| n.starts_with("ran-"))
        .collect();
    names.sort();
    for n in &names {
        fs::remove_file(dir.join(n)).unwrap();
    }
    names
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Each run that needs Ruby: without --no-ruby a stand-in starts (so the check below can see a
/// spawn); with --no-ruby or RBCOP_NO_RUBY=1 none does and `check` holds.
fn refuses(name: &str, config: &str, args: &[&str], check: impl Fn(&Path, &Output)) {
    let dir = project(name, config);
    rbcop(&dir, &[], args);
    assert_ne!(ran(&dir), Vec::<String>::new(), "{name}: the stand-ins are not reached without --no-ruby");
    let flag: Vec<&str> = ["--no-ruby"].iter().chain(args).copied().collect();
    for (env, args) in [(&[][..], &flag[..]), (&[("RBCOP_NO_RUBY", "1")][..], args)] {
        let out = rbcop(&dir, env, args);
        assert_eq!(ran(&dir), Vec::<String>::new(), "{name} {env:?}: {}", text(&out.stderr));
        check(&dir, &out);
    }
    let _ = fs::remove_dir_all(&dir);
}

fn fails(stderr: impl Into<String>) -> impl Fn(&Path, &Output) {
    let stderr = stderr.into();
    move |dir, out| {
        let want = stderr.replace("DIR", &dir.display().to_string());
        assert_eq!((text(&out.stderr), out.status.code()), (want, Some(2)));
    }
}

#[test]
fn config_needing_ruby_fails() {
    let forbids = "needs Ruby, which --no-ruby (or RBCOP_NO_RUBY=1) forbids\n";
    refuses(
        "erb",
        "AllCops:\n  NewCops: <%= 'enable' %>\n",
        &["-f", "e"],
        fails(&format!("rbcop: --no-ruby: DIR/.rubocop.yml: ERB in config {forbids}")),
    );
    refuses(
        "remote",
        "inherit_from: https://example.invalid/rubocop.yml\n",
        &["-f", "e"],
        fails(&format!("rbcop: --no-ruby: remote inherit_from https://example.invalid/rubocop.yml {forbids}")),
    );
    refuses(
        "inherit-gem",
        "inherit_gem:\n  some-gem: config.yml\n",
        &["-f", "e"],
        fails(&format!("rbcop: --no-ruby: finding the some-gem gem {forbids}")),
    );
    refuses(
        "project-index",
        "AllCops:\n  UseProjectIndex: true\n  ProjectIndexIncludesGems: true\n",
        &["-f", "e"],
        fails(&format!("rbcop: --no-ruby: AllCops/ProjectIndexIncludesGems {forbids}")),
    );
}

#[test]
fn ruby_options_fail() {
    for (args, refusal) in [
        (&["-r", "./no_foo.rb", "-f", "e"][..], "rbcop: --require loads Ruby code, which rbcop cannot run\n"),
        (&["--plugin", "rubocop-sorbet"], "rbcop: --plugin loads Ruby code, which rbcop cannot run\n"),
        (&["-f", "MyFormatter"], "rbcop: custom formatter classes are not supported by rbcop: MyFormatter\n"),
    ] {
        refuses("options", "", args, fails(refusal));
    }
}

/// What the RuboCop fallback would run is skipped, said on stderr and listed in `-f json`; the
/// rest of the run is as without it.
#[test]
fn fallback_cops_are_skipped_and_reported() {
    let tail = |dir: &Path, out: &Output, stderr: &str, json: &str| {
        let want = stderr.replace("DIR", &dir.display().to_string());
        assert_eq!(text(&out.stderr), want);
        assert!(text(&out.stdout).ends_with(json), "{}", text(&out.stdout));
        assert_eq!(out.status.code(), Some(0));
    };
    let skipped = "rbcop: not running cops from Ruby code the config requires; --no-ruby keeps RuboCop from \
                   running them\n";
    refuses("custom", "require:\n  - ./no_foo.rb\n", &["-f", "j", "a.rb"], |dir, out| {
        tail(dir, out, skipped, "\"skipped_cops\":[],\"skipped_ruby_code\":true}")
    });
    // A plugin rbcop does not implement: finding its gem needs Ruby, so its cops are not known.
    refuses("plugin", "plugins:\n  - rubocop-fake\n", &["-f", "j", "a.rb"], |dir, out| {
        let warning = "Warning: rbcop: --no-ruby: finding rubocop-fake (plugins:/require:) needs Ruby, which \
                       --no-ruby (or RBCOP_NO_RUBY=1) forbids\n";
        tail(dir, out, &format!("{warning}{skipped}"), "\"skipped_cops\":[],\"skipped_ruby_code\":true}")
    });
}

/// The members are there under --no-ruby with nothing skipped, and never without it.
#[test]
fn json_members_only_under_no_ruby() {
    let dir = project("json", "");
    let out = rbcop(&dir, &[], &["-f", "j", "a.rb"]);
    assert!(text(&out.stdout).ends_with("\"inspected_file_count\":1}}"), "{}", text(&out.stdout));
    let out = rbcop(&dir, &[], &["--no-ruby", "-f", "j", "a.rb"]);
    assert!(
        text(&out.stdout).ends_with("\"inspected_file_count\":1},\"skipped_cops\":[],\"skipped_ruby_code\":false}"),
        "{}",
        text(&out.stdout)
    );
    assert_eq!((text(&out.stderr), out.status.code()), (String::new(), Some(0)));
    assert_eq!(ran(&dir), Vec::<String>::new());
    let _ = fs::remove_dir_all(&dir);
}
