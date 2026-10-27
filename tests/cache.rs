//! The result cache (#281): a warm run prints exactly what a cold one and `--cache false` print.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbcop-cache-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("lib")).unwrap();
    // Physical path: the cache refuses a root under a symlink (/tmp on macOS).
    let dir = dir.canonicalize().unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: enable\n  SuggestExtensions: false\n").unwrap();
    fs::write(dir.join("a.rb"), "x = 1\nputs x  \n").unwrap();
    // CRLF, a multi-line message, non-ASCII, and a file with no offenses.
    fs::write(dir.join("lib/crlf.rb"), "# frozen_string_literal: true\r\n\r\nputs \"é\" ;\r\n").unwrap();
    fs::write(dir.join("lib/heredoc.rb"), "y=<<~A\n  a\\nb\n  A\nputs y\n").unwrap();
    fs::write(dir.join("lib/bad.rb"), b"# frozen_string_literal: true\n\nputs \"\xff\"\n").unwrap();
    fs::write(dir.join("lib/clean.rb"), "# frozen_string_literal: true\n\nputs 1\n").unwrap();
    fs::write(dir.join("lib/syntax.rb"), "def (\n").unwrap();
    dir
}

fn rbcop(dir: &Path, args: &[&str]) -> Output {
    common::command(env!("CARGO_BIN_EXE_rbcop"))
        .env("RUBOCOP_CACHE_ROOT", dir.join("cache-root"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn entries(dir: &Path) -> usize {
    let top = dir.join("cache-root/rbcop_cache");
    below(&top).iter().flat_map(|a| below(a)).flat_map(|b| below(&b)).count()
}

fn below(d: &Path) -> Vec<PathBuf> {
    fs::read_dir(d).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

fn assert_same(a: &Output, b: &Output, what: &str) {
    assert_eq!(String::from_utf8_lossy(&a.stdout), String::from_utf8_lossy(&b.stdout), "{what}: stdout");
    assert_eq!(String::from_utf8_lossy(&a.stderr), String::from_utf8_lossy(&b.stderr), "{what}: stderr");
    assert_eq!(a.status.code(), b.status.code(), "{what}: status");
}

#[test]
fn warm_run_prints_what_a_cold_run_prints() {
    let dir = project("warm");
    for format in ["progress", "json", "emacs", "offenses", "html"] {
        let base = rbcop(&dir, &["--cache", "false", "-f", format]);
        let cold = rbcop(&dir, &["-f", format]);
        let warm = rbcop(&dir, &["-f", format]);
        assert_same(&base, &cold, format);
        assert_same(&base, &warm, format);
    }
    // Every file is cached (the Gemfile too), the one that does not parse included.
    // Twice: the JSON formatter's entries keep Offense#corrections, so they live apart from the
    // other formatters' (src/main.rs, the cache context).
    assert_eq!(entries(&dir), 14);
    // Suppressed offenses, with a directive's justification, under --display-suppressed.
    fs::write(
        dir.join("lib/suppressed.rb"),
        "# frozen_string_literal: true\n\nz = 1 # rubocop:disable Lint/UselessAssignment -- tab\there\n",
    )
    .unwrap();
    for format in ["json", "emacs"] {
        let args = ["--display-suppressed", "-f", format];
        let base = rbcop(&dir, &[&["--cache", "false"][..], &args].concat());
        assert_same(&base, &rbcop(&dir, &args), format);
        assert_same(&base, &rbcop(&dir, &args), format);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// ResultCache#write_cache_file (RuboCop 1.92.1): a cache directory that refuses the entry warns
/// for each file it would save and the run goes on; a cache hit warns nothing.
#[test]
fn unwritable_cache_directory_warns() {
    use std::os::unix::fs::PermissionsExt;
    let dir = project("unwritable");
    rbcop(&dir, &["-f", "e", "a.rb"]);
    let top = dir.join("cache-root/rbcop_cache");
    let contexts: Vec<PathBuf> = below(&top).iter().flat_map(|b| below(b)).collect();
    for c in &contexts {
        fs::set_permissions(c, fs::Permissions::from_mode(0o555)).unwrap();
    }
    // Root writes anyway: nothing to check.
    if fs::write(contexts[0].join("probe"), "").is_err() {
        fs::write(dir.join("b.rb"), "y = 1\n").unwrap();
        let out = rbcop(&dir, &["-f", "e", "a.rb", "b.rb"]);
        assert_same(
            &rbcop(&dir, &["--cache", "false", "-f", "e", "a.rb", "b.rb"]),
            &Output { stderr: vec![], ..out.clone() },
            "warned",
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        let want = "Couldn't write cache file. Continuing without cache.\n  Permission denied @ rb_sysopen - ";
        assert!(stderr.starts_with(want) && stderr.matches("Couldn't").count() == 1, "{stderr}");
    }
    for c in &contexts {
        fs::set_permissions(c, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn config_change_invalidates() {
    let dir = project("config");
    let before = rbcop(&dir, &["-f", "emacs"]);
    assert!(String::from_utf8_lossy(&before.stdout).contains("Layout/TrailingWhitespace"));
    // An inherited file changes the per-file config: the cached offenses must not be reused.
    fs::write(dir.join("base.yml"), "Layout/TrailingWhitespace:\n  Enabled: false\n").unwrap();
    fs::write(
        dir.join(".rubocop.yml"),
        "inherit_from: base.yml\nAllCops:\n  NewCops: enable\n  SuggestExtensions: false\n",
    )
    .unwrap();
    let after = rbcop(&dir, &["-f", "emacs"]);
    assert_same(&rbcop(&dir, &["--cache", "false", "-f", "emacs"]), &after, "after config change");
    assert!(!String::from_utf8_lossy(&after.stdout).contains("Layout/TrailingWhitespace"));
    // So does an edited file, and an option that selects cops.
    fs::write(dir.join("a.rb"), "x = 2\nputs x\n").unwrap();
    assert_same(&rbcop(&dir, &["--cache", "false", "-f", "e"]), &rbcop(&dir, &["-f", "e"]), "edited file");
    let only = ["--only", "Style/FrozenStringLiteralComment", "-f", "e"];
    assert_same(&rbcop(&dir, &[&["--cache", "false"][..], &only].concat()), &rbcop(&dir, &only), "--only");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn autocorrect_on_a_warm_cache_still_corrects() {
    let dir = project("autocorrect");
    rbcop(&dir, &[]);
    let n = entries(&dir);
    let fixed = rbcop(&dir, &["-A", "-f", "e"]);
    assert!(String::from_utf8_lossy(&fixed.stdout).contains("[Corrected]"));
    assert_eq!(fs::read_to_string(dir.join("a.rb")).unwrap(), "# frozen_string_literal: true\n\nx = 1\nputs x\n");
    // The corrected files' new contents were not saved (their offenses are not a plain run's).
    assert_eq!(entries(&dir), n);
    assert_same(&rbcop(&dir, &["--cache", "false"]), &rbcop(&dir, &[]), "after -a");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pruning_never_follows_a_symlink_in_the_cache_tree() {
    let dir = project("prune");
    fs::write(
        dir.join(".rubocop.yml"),
        "AllCops:\n  NewCops: enable\n  SuggestExtensions: false\n  MaxFilesInCache: 1\n",
    )
    .unwrap();
    rbcop(&dir, &[]);
    // Files outside the cache, reachable through a planted directory link and a planted entry link.
    let outside = dir.join("outside");
    fs::create_dir_all(outside.join("b/c")).unwrap();
    fs::write(outside.join("b/c/victim"), "keep").unwrap();
    fs::write(outside.join("victim2"), "keep").unwrap();
    let top = dir.join("cache-root/rbcop_cache");
    std::os::unix::fs::symlink(&outside, top.join("evil")).unwrap();
    let entry_dir = below(&below(&top).into_iter().find(|p| !p.is_symlink()).unwrap())[0].clone();
    std::os::unix::fs::symlink(outside.join("victim2"), entry_dir.join("link")).unwrap();
    for _ in 0..3 {
        fs::write(dir.join("a.rb"), format!("{}\n", fs::read_to_string(dir.join("a.rb")).unwrap())).unwrap();
        rbcop(&dir, &[]);
    }
    assert_eq!(fs::read_to_string(outside.join("b/c/victim")).unwrap(), "keep");
    assert_eq!(fs::read_to_string(outside.join("victim2")).unwrap(), "keep");
    // Pruning did run on the real entries.
    assert!(entries(&dir) < 7, "{}", entries(&dir));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn stdin_and_cache_false_never_touch_the_cache() {
    let dir = project("stdin");
    let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .env("RUBOCOP_CACHE_ROOT", dir.join("cache-root"))
        .args(["--stdin", "a.rb"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(b"x = 1\n").unwrap();
    child.wait().unwrap();
    rbcop(&dir, &["--cache", "false"]);
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  UseCache: false\n").unwrap();
    rbcop(&dir, &[]);
    assert!(!dir.join("cache-root").exists());
    // -P needs the cache.
    let out = rbcop(&dir, &["-P"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "-P/--parallel uses caching to speed up execution, so combining with AllCops: UseCache: false is not \
         allowed.\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn project_index_change_invalidates() {
    // With AllCops/UseProjectIndex, a file's offenses depend on the other files (#295): editing
    // b.rb must invalidate a.rb's cached report (ProjectIndexHelp#external_dependency_checksum).
    let dir = project("index");
    fs::write(
        dir.join(".rubocop.yml"),
        "AllCops:\n  UseProjectIndex: true\n  NewCops: enable\n  SuggestExtensions: false\n",
    )
    .unwrap();
    let base =
        |n: u8| format!("# frozen_string_literal: true\n\n# Doc.\nclass Base\n  def get_name\n    {n}\n  end\nend\n");
    fs::write(dir.join("a.rb"), base(1)).unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nX = 1\n").unwrap();
    let args = ["--only", "Lint/DuplicateMethods", "-f", "emacs", "a.rb"];
    assert_eq!(rbcop(&dir, &args).status.code(), Some(0));
    fs::write(dir.join("b.rb"), base(2)).unwrap();
    let warm = rbcop(&dir, &args);
    assert_same(&rbcop(&dir, &[&["--cache", "false"][..], &args].concat()), &warm, "index");
    let out = String::from_utf8_lossy(&warm.stdout);
    assert!(out.contains("Method `Base#get_name` is defined at both b.rb:5 and a.rb:5."), "{out}");
    let _ = fs::remove_dir_all(&dir);
}
