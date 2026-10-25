//! `-d/--debug`, `--profile` and `--memory` (#297). The `-d` output was recorded with RuboCop
//! 1.91.0 on the same files, with the paths of its config/default.yml files replaced by rbcop's
//! built-in labels (docs/known-gaps.md) and the time left out.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbcop-debug-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    let dir = dir.canonicalize().unwrap();
    for (file, text) in [
        (
            ".rubocop.yml",
            "plugins:\n  - rubocop-performance\ninherit_from: base.yml\nAllCops:\n  NewCops: disable\n\
             Style/StringLiterals:\n  EnforcedStyle: double_quotes\n",
        ),
        ("base.yml", "Style/StringLiterals:\n  EnforcedStyle: single_quotes\n"),
        ("Gemfile", ""),
        ("a.rb", "x = 1\nputs x\n"),
        ("sub/.rubocop.yml", "Layout/IndentationWidth:\n  Width: 2\n"),
        ("sub/b.rb", "def f(a)\n  a\nend\n"),
    ] {
        fs::write(dir.join(file), text).unwrap();
    }
    dir
}

/// stdout with the project directory as `<dir>` and the run time as `<t>`.
fn run(dir: &Path, args: &[&str]) -> String {
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(args)
        .current_dir(dir)
        .env("RUBOCOP_CACHE_ROOT", dir.join("cache"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    let stdout = String::from_utf8_lossy(&out.stdout).replace(&dir.display().to_string(), "<dir>");
    let (head, time) = stdout.trim_end().rsplit_once("Finished in ").unwrap();
    assert!(time.ends_with(" seconds") && time[..time.len() - 8].parse::<f64>().is_ok(), "{time}");
    format!("{head}Finished in <t> seconds\n")
}

const CONFIG: &str = "For <dir>: configuration from <dir>/.rubocop.yml
Default configuration from <rbcop built-in>/rubocop/config/default.yml
Plugin configuration from <rbcop built-in>/rubocop-performance/config/default.yml
Inheriting configuration from <dir>/base.yml
.rubocop.yml: Style/StringLiterals:EnforcedStyle overrides the same parameter in base.yml
";

const SUB_CONFIG: &str = "For <dir>/sub: configuration from <dir>/sub/.rubocop.yml
AllCops/Exclude configuration from <dir>/.rubocop.yml
Inheriting configuration from <dir>/base.yml
.rubocop.yml: Style/StringLiterals:EnforcedStyle overrides the same parameter in base.yml
";

const REPORT: &str = "

Offenses:

Gemfile:1:1: W: Lint/EmptyFile: Empty file detected.
a.rb:1:1: C: [Correctable] Style/FrozenStringLiteralComment: Missing frozen string literal comment.
x = 1
^
sub/b.rb:1:1: C: [Correctable] Style/FrozenStringLiteralComment: Missing frozen string literal comment.
def f(a)
^
sub/b.rb:1:7: C: Naming/MethodParameterName: Method parameter must be at least 3 characters long.
def f(a)
      ^

3 files inspected, 4 offenses detected, 2 offenses autocorrectable
Finished in <t> seconds
";

/// Each config file as it loads (a cached file is loaded, and named, again), the defaults and
/// each plugin's once, an inherited setting that is overridden, and each file as its turn comes.
/// With `--cache` given RuboCop inspects one file after another, so a directory's config loads
/// when its first file is inspected.
#[test]
fn debug_serial() {
    let dir = project("serial");
    let want = format!(
        "{CONFIG}Inspecting 3 files\nScanning <dir>/Gemfile\nWScanning <dir>/a.rb\nCScanning <dir>/sub/b.rb\n{SUB_CONFIG}C{REPORT}"
    );
    assert_eq!(run(&dir, &["-d", "--cache", "false"]), want);

    let want = format!(
        "{CONFIG}Ignoring AllCops/Exclude from parent foldersInspecting 1 file\nScanning <dir>/sub/b.rb\n\
         For <dir>/sub: configuration from <dir>/sub/.rubocop.yml\nIgnoring AllCops/Exclude from parent foldersC\n\n\
         Offenses:\n\n\
         sub/b.rb:1:1: C: [Correctable] Style/FrozenStringLiteralComment: Missing frozen string literal comment.\n\
         def f(a)\n^\n\
         sub/b.rb:1:7: C: Naming/MethodParameterName: Method parameter must be at least 3 characters long.\n\
         def f(a)\n      ^\n\n\
         1 file inspected, 2 offenses detected, 1 offense autocorrectable\nFinished in <t> seconds\n"
    );
    assert_eq!(run(&dir, &["-d", "--cache", "false", "--ignore-parent-exclusion", "sub/b.rb"]), want);
    let _ = fs::remove_dir_all(&dir);
}

/// With only options like `-d` given RuboCop inspects in parallel; its workers' output comes
/// before the files' (here, with fewer files than workers, as RuboCop prints it), and a report
/// read from the result cache is named.
#[test]
fn debug_parallel() {
    let dir = project("parallel");
    let want = format!(
        "{CONFIG}Use parallel by default.\nInspecting 3 files\nRunning parallel inspection\n{SUB_CONFIG}\
         Scanning <dir>/Gemfile\nScanning <dir>/a.rb\nScanning <dir>/sub/b.rb\nWCC{REPORT}"
    );
    assert_eq!(run(&dir, &["-d"]), want);
    let second = run(&dir, &["-d"]);
    let loaded: Vec<&str> =
        second.lines().filter(|l| l.starts_with("Loading cache from <dir>/cache/rbcop_cache/")).collect();
    assert_eq!(loaded.len(), 3, "{second}");
    let _ = fs::remove_dir_all(&dir);
}

/// Deliberate difference: rbcop's `--profile` report is the wall time spent in each cop, and
/// `--memory`'s its peak resident set size (RuboCop's are StackProf and MemoryProfiler reports
/// of the Ruby VM). `--memory` alone does nothing, as in RuboCop.
#[test]
fn profile_and_memory() {
    let dir = project("profile");
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--profile", "--memory", "a.rb"])
        .current_dir(&dir)
        .env("RUBOCOP_CACHE_ROOT", dir.join("cache"))
        .output()
        .unwrap();
    let tmp = dir.join("tmp");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.ends_with(&format!(
            "Profile report generated at {}\nBuilding memory report...\nMemory report generated at {}\n",
            tmp.join("rbcop-profile.txt").display(),
            tmp.join("rbcop-memory.txt").display()
        )),
        "{stdout}"
    );
    let profile = fs::read_to_string(tmp.join("rbcop-profile.txt")).unwrap();
    assert!(profile.lines().any(|l| l.ends_with("  Style/FrozenStringLiteralComment")), "{profile}");
    assert!(fs::read_to_string(tmp.join("rbcop-memory.txt")).unwrap().starts_with("rbcop --memory: peak resident"));
    // `--profile` turns the result cache off unless `--cache` is given.
    assert!(!dir.join("cache").exists());

    fs::remove_dir_all(&tmp).unwrap();
    let out =
        common::command(env!("CARGO_BIN_EXE_rbcop")).args(["--memory", "a.rb"]).current_dir(&dir).output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("report"));
    assert!(!tmp.exists());
    let _ = fs::remove_dir_all(&dir);
}
