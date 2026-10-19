//! Regressions from the core infrastructure review: file I/O, discovery and config edge cases the
//! corpora do not reach. Expected behaviour is RuboCop 1.91's unless a test says otherwise.

mod common;

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Output;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbcop-core-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    // Physical path, as the binary reports it (/tmp is a symlink on macOS).
    let dir = dir.canonicalize().unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: disable\n").unwrap();
    dir
}

fn rbcop(dir: &Path, args: &[&str]) -> Output {
    common::command(env!("CARGO_BIN_EXE_rbcop")).args(args).current_dir(dir).output().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const SPACING: &[&str] = &["--only", "Layout/ExtraSpacing,Layout/SpaceAroundOperators", "-f", "e"];

fn root() -> bool {
    // SAFETY: no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// -a rewrites through a temporary file and a rename, yet keeps what RuboCop's in-place write
/// keeps: the mode, a symlink (its target is corrected), a hard link (both names see the new
/// content); line endings come out as LF, as RuboCop writes them. No temporary file is left.
#[test]
fn autocorrect_write_keeps_mode_symlinks_and_hard_links() {
    let dir = scratch("write");
    fs::write(dir.join("m.rb"), "x  =  1\r\n").unwrap();
    fs::set_permissions(dir.join("m.rb"), fs::Permissions::from_mode(0o751)).unwrap();
    fs::write(dir.join("target.rb"), "x  =  1\n").unwrap();
    symlink("target.rb", dir.join("link.rb")).unwrap();
    fs::write(dir.join("h.rb"), "x  =  1\n").unwrap();
    fs::hard_link(dir.join("h.rb"), dir.join("h2.rb")).unwrap();
    let mut args = vec!["-a"];
    args.extend(SPACING);
    args.extend(["m.rb", "link.rb", "h.rb"]);
    let out = rbcop(&dir, &args);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(fs::read_to_string(dir.join("m.rb")).unwrap(), "x = 1\n");
    assert_eq!(fs::metadata(dir.join("m.rb")).unwrap().mode() & 0o7777, 0o751);
    assert!(fs::symlink_metadata(dir.join("link.rb")).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(dir.join("target.rb")).unwrap(), "x = 1\n");
    assert_eq!(fs::read_to_string(dir.join("h2.rb")).unwrap(), "x = 1\n");
    assert_eq!(fs::metadata(dir.join("h.rb")).unwrap().nlink(), 2);
    let leftovers: Vec<_> =
        fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("rbcop-")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// A file -a cannot write (or read) is an error that ends the run with status 2, as RuboCop's
/// Errno::EACCES does; its offenses are not reported as corrected, and the file is unchanged even
/// though its directory is writable.
#[test]
fn autocorrect_unwritable_or_unreadable_file_is_an_error() {
    if root() {
        return;
    }
    let dir = scratch("eacces");
    for (name, mode) in [("ro.rb", 0o444), ("wo.rb", 0o200), ("none.rb", 0o000)] {
        fs::write(dir.join(name), "x  =  1\n").unwrap();
        fs::set_permissions(dir.join(name), fs::Permissions::from_mode(mode)).unwrap();
        for autocorrect in [true, false] {
            let mut args = if autocorrect { vec!["-a"] } else { vec![] };
            args.extend(SPACING);
            args.push(name);
            let out = rbcop(&dir, &args);
            if !autocorrect && name == "ro.rb" {
                assert_eq!(out.status.code(), Some(1), "{name}");
                continue;
            }
            assert_eq!(out.status.code(), Some(2), "{name} -a={autocorrect}");
            assert_eq!(
                text(&out.stderr).trim(),
                format!("Permission denied @ rb_sysopen - {}", dir.join(name).display()),
                "{name} -a={autocorrect}"
            );
            assert!(!text(&out.stdout).contains("Corrected"), "{name}");
        }
        fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(fs::read_to_string(dir.join(name)).unwrap(), "x  =  1\n", "{name}");
    }
    // The run stops there: a later file is not corrected either.
    fs::write(dir.join("later.rb"), "x  =  1\n").unwrap();
    fs::set_permissions(dir.join("ro.rb"), fs::Permissions::from_mode(0o444)).unwrap();
    let mut args = vec!["-a"];
    args.extend(SPACING);
    args.extend(["ro.rb", "later.rb"]);
    assert_eq!(rbcop(&dir, &args).status.code(), Some(2));
    assert_eq!(fs::read_to_string(dir.join("later.rb")).unwrap(), "x  =  1\n");
    let _ = fs::remove_dir_all(&dir);
}

/// `-F -a` stops at the first failing file: files after it are neither reported nor corrected.
#[test]
fn fail_fast_autocorrect_leaves_later_files_alone() {
    let dir = scratch("failfast");
    fs::write(dir.join("a.rb"), "x  =  1\nbegin; rescue Exception; end\n").unwrap();
    fs::write(dir.join("b.rb"), "y  =  2\n").unwrap();
    let out =
        rbcop(&dir, &["-F", "-a", "--only", "Layout/ExtraSpacing,Lint/RescueException", "-f", "e", "a.rb", "b.rb"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!text(&out.stdout).contains("b.rb"));
    assert_eq!(fs::read_to_string(dir.join("a.rb")).unwrap(), "x = 1\nbegin; rescue Exception; end\n");
    assert_eq!(fs::read_to_string(dir.join("b.rb")).unwrap(), "y  =  2\n");
    let _ = fs::remove_dir_all(&dir);
}

/// One file given twice (through a symlink) is corrected once, in order: the second path sees the
/// corrected file and has nothing to report.
#[test]
fn autocorrect_same_file_twice_is_sequential() {
    let dir = scratch("twice");
    fs::write(dir.join("real.rb"), "z  =  3\n").unwrap();
    symlink("real.rb", dir.join("link.rb")).unwrap();
    let mut args = vec!["-a"];
    args.extend(SPACING);
    args.extend(["link.rb", "real.rb"]);
    let out = rbcop(&dir, &args);
    let stdout = text(&out.stdout);
    assert_eq!(stdout.lines().filter(|l| l.contains("link.rb:")).count(), 3, "{stdout}");
    assert!(!stdout.contains("real.rb:"), "{stdout}");
    assert_eq!(fs::read_to_string(dir.join("real.rb")).unwrap(), "z = 3\n");
    let _ = fs::remove_dir_all(&dir);
}

/// A source that is not valid UTF-8 (and names no other encoding) gets only Lint/Syntax, even when
/// the bad bytes are after `__END__`; a magic comment naming another encoding makes it valid.
#[test]
fn invalid_utf8_is_a_syntax_error() {
    let dir = scratch("utf8");
    fs::write(dir.join("bad.rb"), b"x  =  1\n__END__\n\xff\n").unwrap();
    fs::write(dir.join("latin.rb"), b"# encoding: iso-8859-1\nx  =  \"\xe9\"\n").unwrap();
    for autocorrect in [false, true] {
        let mut args = if autocorrect { vec!["-a"] } else { vec![] };
        args.extend(["--only", "Layout/ExtraSpacing", "-f", "e", "bad.rb", "latin.rb"]);
        let out = rbcop(&dir, &args);
        let stdout = text(&out.stdout);
        let bad: Vec<_> = stdout.lines().filter(|l| l.contains("bad.rb")).collect();
        assert_eq!(bad, [format!("{}/bad.rb:1:1: F: Lint/Syntax: Invalid byte sequence in utf-8.", dir.display())]);
        assert!(stdout.contains("latin.rb:2:2: C:"), "{stdout}");
    }
    assert_eq!(fs::read(dir.join("bad.rb")).unwrap(), b"x  =  1\n__END__\n\xff\n");
    let _ = fs::remove_dir_all(&dir);
}

/// A magic comment naming no encoding: RuboCop refuses the file, so -A must not rewrite it (#246).
#[test]
fn unknown_encoding_name_is_not_corrected() {
    let dir = scratch("unknown_encoding");
    let src = b"# encoding: utf-8names = Array.new\nx = 1\n";
    fs::write(dir.join("t.rb"), src).unwrap();
    let out = rbcop(&dir, &["-A", "--force-default-config", "-f", "e", "t.rb"]);
    let want = format!("{}/t.rb:1:1: F: Lint/Syntax: Unknown encoding name - utf-8names.\n", dir.display());
    assert_eq!(text(&out.stdout).lines().next().map(|l| format!("{l}\n")), Some(want));
    assert_eq!(fs::read(dir.join("t.rb")).unwrap(), src);
    let _ = fs::remove_dir_all(&dir);
}

/// An inherit_from cycle is reported as an error (RuboCop overflows its stack; rbcop used to abort).
#[test]
fn inherit_from_cycle_is_an_error() {
    let dir = scratch("cycle");
    fs::write(dir.join(".rubocop.yml"), "inherit_from: b.yml\n").unwrap();
    fs::write(dir.join("b.yml"), "inherit_from: c.yml\n").unwrap();
    fs::write(dir.join("c.yml"), "inherit_from: b.yml\n").unwrap();
    fs::write(dir.join("a.rb"), "x = 1\n").unwrap();
    let out = rbcop(&dir, &["a.rb"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("inherit_from cycle"), "{}", text(&out.stderr));
    let _ = fs::remove_dir_all(&dir);
}

/// inherit_from accepts globs: Dir.glob from the working directory, each match then taken
/// relative to the inheriting file (a later entry still wins over an earlier one).
#[test]
fn inherit_from_glob() {
    let dir = scratch("glob");
    fs::create_dir_all(dir.join("cfg")).unwrap();
    fs::write(dir.join(".rubocop.yml"), "inherit_from:\n  - cfg/*.yml\n  - other.yml\nAllCops:\n  NewCops: disable\n")
        .unwrap();
    fs::write(dir.join("cfg/one.yml"), "Style/StringLiterals:\n  EnforcedStyle: double_quotes\n").unwrap();
    fs::write(dir.join("cfg/two.yml"), "Layout/LineLength:\n  Max: 5\n").unwrap();
    fs::write(dir.join("other.yml"), "Layout/LineLength:\n  Max: 7\n").unwrap();
    fs::write(dir.join("a.rb"), "x = 'a'\n").unwrap();
    let out = rbcop(&dir, &["--only", "Layout/LineLength,Style/StringLiterals", "-f", "e", "a.rb"]);
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    assert!(stdout.contains("Style/StringLiterals: Prefer double-quoted"), "{stdout}");
    assert!(!stdout.contains("LineLength"), "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}

/// Directory walks keep symlinked files and follow symlinked directories, except into an excluded
/// directory or a loop.
#[test]
fn target_finder_follows_symlinks() {
    let dir = scratch("symlinks");
    let outside = scratch("symlinks-outside");
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: disable\n  Exclude:\n    - excl/**/*\n").unwrap();
    fs::create_dir_all(dir.join("lib")).unwrap();
    fs::create_dir_all(dir.join("excl")).unwrap();
    fs::create_dir_all(outside.join("deep")).unwrap();
    fs::write(dir.join("lib/a.rb"), "").unwrap();
    fs::write(dir.join("excl/e.rb"), "").unwrap();
    fs::write(outside.join("o.rb"), "").unwrap();
    fs::write(outside.join("deep/d.rb"), "").unwrap();
    symlink(outside.join("o.rb"), dir.join("lib/link.rb")).unwrap();
    symlink(&outside, dir.join("linkdir")).unwrap();
    symlink("..", dir.join("lib/loop")).unwrap();
    symlink("excl", dir.join("exlink")).unwrap();
    let out = rbcop(&dir, &["-L"]);
    let mut listed: Vec<&str> =
        std::str::from_utf8(&out.stdout).unwrap().lines().filter(|l| l.ends_with(".rb")).collect();
    listed.sort();
    assert_eq!(listed, ["lib/a.rb", "lib/link.rb", "linkdir/deep/d.rb", "linkdir/o.rb"]);
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&outside);
}

/// A cycle through two symlinked directories is entered once, not expanded until ELOOP, and a
/// deep directory tree is walked without recursion.
#[test]
fn target_finder_stops_at_symlink_cycles_and_survives_deep_trees() {
    let dir = scratch("cycles");
    fs::create_dir_all(dir.join("d1")).unwrap();
    fs::create_dir_all(dir.join("d2")).unwrap();
    symlink("../d2", dir.join("d1/to2")).unwrap();
    symlink("../d1", dir.join("d2/to1")).unwrap();
    fs::write(dir.join("d1/a.rb"), "").unwrap();
    let mut deep = dir.join("deep");
    for _ in 0..400 {
        deep.push("d");
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("z.rb"), "").unwrap();
    let out = rbcop(&dir, &["-L"]);
    let listed: Vec<&str> = std::str::from_utf8(&out.stdout).unwrap().lines().filter(|l| l.ends_with(".rb")).collect();
    assert_eq!(listed.len(), 3, "{listed:?}");
    assert!(listed.contains(&"d1/a.rb") && listed.contains(&"d2/to1/a.rb"), "{listed:?}");
    assert!(listed.iter().any(|l| l.ends_with("/d/z.rb")));
    let _ = fs::remove_dir_all(&dir);
}

/// The temporary file behind an atomic -a write never has a wider mode than the file it replaces,
/// and a pre-existing symlink at a temp-like name is not followed.
#[test]
fn autocorrect_temp_file_does_not_widen_access() {
    let dir = scratch("tmpmode");
    fs::write(dir.join("secret.rb"), "x  =  1\n").unwrap();
    fs::set_permissions(dir.join("secret.rb"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(dir.join("victim"), "keep\n").unwrap();
    symlink("victim", dir.join(format!(".secret.rb.rbcop-{}-0.tmp", std::process::id()))).unwrap();
    let mut args = vec!["-a"];
    args.extend(SPACING);
    args.push("secret.rb");
    assert_eq!(rbcop(&dir, &args).status.code(), Some(0));
    assert_eq!(fs::metadata(dir.join("secret.rb")).unwrap().mode() & 0o777, 0o600);
    assert_eq!(fs::read_to_string(dir.join("victim")).unwrap(), "keep\n");
    let _ = fs::remove_dir_all(&dir);
}

/// TargetRubyVersion sources (.ruby-version, a lone gemspec) are searched up to `/`, not only up to
/// the project root.
#[test]
fn target_ruby_found_above_project_root() {
    let top = scratch("target-ruby");
    fs::remove_file(top.join("Gemfile")).unwrap();
    fs::write(top.join(".ruby-version"), "3.1.2\n").unwrap();
    let proj = top.join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("Gemfile"), "").unwrap();
    let out = rbcop(&proj, &["-V"]);
    assert!(text(&out.stdout).contains("analyzing as Ruby 3.1,"), "{}", text(&out.stdout));
    let _ = fs::remove_dir_all(&top);
}

/// A failed write to an -o file (here a file size limit) fails the run like RuboCop's raise,
/// instead of leaving a truncated report behind a normal exit status.
#[test]
fn output_file_write_failure_is_an_error() {
    use std::os::unix::process::CommandExt;
    let dir = scratch("efbig");
    fs::write(dir.join("big.rb"), "x  =  1\n".repeat(3000)).unwrap();
    let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
    cmd.args(["--only", "Layout/ExtraSpacing", "-f", "e", "-o", "out.txt", "big.rb"]).current_dir(&dir);
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
            let limit = libc::rlimit { rlim_cur: 8192, rlim_max: 8192 };
            libc::setrlimit(libc::RLIMIT_FSIZE, &limit);
            Ok(())
        });
    }
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("File too large @ rb_sys_fail_on_write - out.txt"), "{}", text(&out.stderr));
    let _ = fs::remove_dir_all(&dir);
}

/// The `.rubocop` arguments file is split like Shellwords: inside double quotes a backslash only
/// escapes `$ \` " \\` and newline, and an unterminated quote is an error.
#[test]
fn arguments_file_shellwords() {
    let dir = scratch("shellwords");
    fs::write(dir.join(".rubocop"), "-c \"my\\cfg.yml\"\n").unwrap();
    let out = rbcop(&dir, &["x.rb"]);
    assert!(text(&out.stderr).contains(&format!("{}/my\\cfg.yml", dir.display())), "{}", text(&out.stderr));
    fs::write(dir.join(".rubocop"), "-c \"my.yml\n").unwrap();
    let out = rbcop(&dir, &["x.rb"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("Unmatched quote"), "{}", text(&out.stderr));
    let _ = fs::remove_dir_all(&dir);
}

/// Directory targets come out in Ruby String order (bytes of the whole path), as TargetFinder's
/// `sort_by!(&:itself)`: `lib/foo-baz.rb` < `lib/foo.rb` < `lib/foo/bar.rb`.
#[test]
fn directory_targets_sort_as_strings() {
    let dir = scratch("order");
    fs::create_dir_all(dir.join("lib/foo")).unwrap();
    for f in ["lib/foo.rb", "lib/foo/bar.rb", "lib/foo-baz.rb"] {
        fs::write(dir.join(f), "x = 1\n").unwrap();
    }
    let out = rbcop(&dir, &["-L"]);
    assert_eq!(text(&out.stdout), "Gemfile\nlib/foo-baz.rb\nlib/foo.rb\nlib/foo/bar.rb\n");
    let _ = fs::remove_dir_all(&dir);
}

/// `--stdin -a/-A` with nothing corrected echoes the source bytes unchanged after the separator,
/// as RuboCop does, even when they are not UTF-8 (an editor replaces its buffer with this output).
#[test]
fn stdin_autocorrect_echoes_original_bytes() {
    use std::io::Write;
    use std::process::Stdio;
    let dir = scratch("stdin-bytes");
    let src: &[u8] = b"x = \"\xff\"\nputs x\n";
    for mode in ["-a", "-A"] {
        let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(["-f", "s", mode, "--stdin", "invalid.rb"])
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(src).unwrap();
        let out = child.wait_with_output().unwrap();
        let sep = b"====================\n";
        let at = out.stdout.windows(sep.len()).position(|w| w == sep).expect("separator");
        assert_eq!(&out.stdout[at + sep.len()..], src, "{mode}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// --editor-mode (RuboCop::LSP.enabled?): the Metrics cops report a definition's head, Lint/Syntax
/// has no parser line, and `AutoCorrect: contextual` cops do not correct. The clang output (ranges
/// as carets) was recorded with RuboCop 1.91 in tests/scenarios/editor_mode.
#[test]
fn editor_mode_ranges_and_contextual_autocorrect() {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios/editor_mode/project");
    let out = rbcop(&project, &["--editor-mode", "-f", "c", "metrics.rb", "a.rb"]);
    let want = fs::read_to_string(project.join("../clang.txt")).unwrap();
    assert_eq!(text(&out.stdout), want);
    assert_eq!(out.status.code(), Some(1));

    let dir = scratch("editor-mode");
    let src = "# frozen_string_literal: true\n\ndef foo(unused)\n  1\nend\nfoo(1)  \nputs 1\n\n\n";
    fs::write(dir.join("a.rb"), src).unwrap();
    let out = rbcop(&dir, &["--editor-mode", "-f", "s", "-a", "a.rb"]);
    assert!(text(&out.stdout).ends_with("1 file inspected, 3 offenses detected, 2 offenses corrected\n"));
    assert_eq!(out.status.code(), Some(1));
    assert!(fs::read_to_string(dir.join("a.rb")).unwrap().contains("def foo(unused)"));
    let _ = fs::remove_dir_all(&dir);
}

/// Config load errors: the first stderr line as RuboCop 1.91 prints it (Ruby's backtrace, which
/// RuboCop prints after it for most of these, is not reproduced), exit 2.
#[test]
fn config_load_errors_worded_like_rubocop() {
    let cases: &[(&str, &str)] = &[
        ("- a\n- b\n", "Error: Malformed configuration in {D}/.rubocop.yml"),
        ("inherit_from: nope.yml\n", "Configuration file not found: {D}/nope.yml"),
        (
            "Style/StringLiterals: [\n",
            "({D}/.rubocop.yml): did not find expected node content while parsing a flow node at line 2 column 1",
        ),
        ("\"x", "({D}/.rubocop.yml): found unexpected end of stream while scanning a quoted scalar at line 1 column 1"),
        ("a: b: c\n", "({D}/.rubocop.yml): mapping values are not allowed in this context at line 1 column 5"),
        (
            "Foo:\n  Bar: 1\n Baz: 2\n",
            "({D}/.rubocop.yml): did not find expected key while parsing a block mapping at line 1 column 1",
        ),
        (
            "a:\n\t- b\n",
            "({D}/.rubocop.yml): found character that cannot start any token while scanning for the next token at line 2 column 1",
        ),
        (
            "a: &\n",
            "({D}/.rubocop.yml): did not find expected alphabetic or numeric character while scanning an anchor at line 1 column 4",
        ),
        // libyaml: every `:` in a flow collection is a value indicator (#527), and a simple key
        // is limited to one line there too (#528).
        (
            "Layout/LineLength:\n  AllowedPatterns: [:foo, :bar]\n",
            "({D}/.rubocop.yml): did not find expected node content while parsing a flow node at line 2 column 21",
        ),
        (
            "Layout/LineLength:\n  X: {foo\n    : bar}\n",
            "({D}/.rubocop.yml): did not find expected ',' or '}' while parsing a flow mapping at line 2 column 6",
        ),
        (
            "a:\n  - b\n\t\n",
            "({D}/.rubocop.yml): found a tab character that violates indentation while scanning a plain scalar at line 2 column 5",
        ),
        // `|`/`>` start no token in a flow collection (#539); `x:` before a flow indicator (#540).
        (
            "Metrics/BlockLength:\n  AllowedMethods: [>a, b]\n",
            "({D}/.rubocop.yml): found character that cannot start any token while scanning for the next token at line 2 column 20",
        ),
        (
            "Metrics/BlockLength: {AllowedMethods:[a]}\n",
            "({D}/.rubocop.yml): found unexpected ':' while scanning a plain scalar at line 1 column 23",
        ),
        // libyaml's problem and mark (#541): no context mark is line 1 column 1; the parser, not
        // the scanner, rejects content after a quoted scalar; anchor names; directives.
        ("  a: 1\nb: 2\n", "({D}/.rubocop.yml): did not find expected <document start> at line 1 column 1"),
        (
            "a: ['b' 'c']\n",
            "({D}/.rubocop.yml): did not find expected ',' or ']' while parsing a flow sequence at line 1 column 4",
        ),
        (
            "a: &b'x'\n",
            "({D}/.rubocop.yml): did not find expected alphabetic or numeric character while scanning an anchor at line 1 column 4",
        ),
        (
            "%Foo\n---\na: 1\n",
            "({D}/.rubocop.yml): found unknown directive name while scanning a directive at line 1 column 1",
        ),
        // YAMLDuplicationChecker reads `value` of each mapping key node during the parse; the
        // unknown alias is ToRuby's, after a syntax error.
        ("a:\n  ? [x]\n  : 1\n", "undefined method 'value' for an instance of Psych::Nodes::Sequence"),
        (
            "a: *zz\nb: [\n",
            "({D}/.rubocop.yml): did not find expected node content while parsing a flow node at line 3 column 1",
        ),
        ("a: *x\n", "An alias referenced an unknown anchor: x"),
        ("b: 2020-01-01\n", "Tried to load unspecified class: Date"),
        ("b: 2020-01-01 10:00:00\n", "Tried to load unspecified class: Time"),
        ("a: !ruby/object:Foo {}\n", "Tried to load unspecified class: Foo"),
        // A list set to `~` is removed by the merge: Naming/PredicatePrefix#validate_config reads nil.
        (
            "AllCops:\n  NewCops: disable\nNaming/PredicatePrefix:\n  NamePrefix: ~\n",
            "undefined method 'include?' for nil",
        ),
        (
            "AllCops:\n  NewCops: disable\nNaming/PredicatePrefix:\n  ForbiddenPrefixes: ~\n",
            "undefined method 'each' for nil",
        ),
        (
            "AllCops:\n  NewCops: disable\nNaming/PredicatePrefix:\n  NamePrefix: 1\n",
            "undefined method 'include?' for an instance of Integer",
        ),
        // libyaml 0.2.5 and Psych's ToRuby (#560, #561, #565, #566, #571, #574, #580): a BOM counts a
        // column, control characters, NEL/LS breaks, `:` before a flow indicator, tags, streams
        // after `...`, directives, block scalar indentation.
        (
            "\u{FEFF}AllCops:\n  SuggestExtensions: false\nLayout/LineLength:\n  Max: 9\n",
            "({D}/.rubocop.yml): did not find expected <document start> at line 1 column 1",
        ),
        (
            "Layout/LineLength:\n  X: [a :]\n",
            "({D}/.rubocop.yml): found unexpected ':' while scanning a plain scalar at line 2 column 7",
        ),
        (
            "Layout/LineLength:\n  X: [a :, b]\n",
            "({D}/.rubocop.yml): found unexpected ':' while scanning a plain scalar at line 2 column 7",
        ),
        (
            "Layout/LineLength:\n  X: {a :}\n",
            "({D}/.rubocop.yml): found unexpected ':' while scanning a plain scalar at line 2 column 7",
        ),
        (
            "Layout/LineLength:\n  X: [a :{b}]\n",
            "({D}/.rubocop.yml): found unexpected ':' while scanning a plain scalar at line 2 column 7",
        ),
        (
            "Layout/LineLength:\n  X: [a b :]\n",
            "({D}/.rubocop.yml): found unexpected ':' while scanning a plain scalar at line 2 column 7",
        ),
        (
            "Layout/LineLength:\n  X: [? : b]\n",
            "({D}/.rubocop.yml): did not find expected ',' or ']' while parsing a flow sequence at line 2 column 6",
        ),
        (
            "AllCops:\n  NewCops: enable\n...\nStyle/Foo: 1\n",
            "({D}/.rubocop.yml): did not find expected <document start> at line 1 column 1",
        ),
        (
            "...\nLayout/LineLength:\n  Max: 9\n",
            "({D}/.rubocop.yml): did not find expected node content while parsing a block node at line 1 column 1",
        ),
        (
            "%TAG ! a\n%TAG ! b\n---\nLayout/LineLength:\n  Max: 9\n",
            "({D}/.rubocop.yml): found duplicate %TAG directive at line 1 column 1",
        ),
        (
            "AllCops:\n  NewCops: enable\n# \x07\n",
            "({D}/.rubocop.yml): control characters are not allowed at line 1 column 1",
        ),
        (
            "Layout/LineLength:\n  X: b\u{007F}\n",
            "({D}/.rubocop.yml): control characters are not allowed at line 1 column 1",
        ),
        (
            "Layout/LineLength:\n  X: \u{FFFE}\n",
            "({D}/.rubocop.yml): control characters are not allowed at line 1 column 1",
        ),
        (
            "Layout/LineLength:\n  X: x\u{2028}y\n",
            "({D}/.rubocop.yml): could not find expected ':' while scanning a simple key at line 3 column 1",
        ),
        (
            "Layout/LineLength:\n  X: x\u{0085}y\n",
            "({D}/.rubocop.yml): could not find expected ':' while scanning a simple key at line 3 column 1",
        ),
        (
            "Layout/LineLength:\n  X: [!b]\n",
            "({D}/.rubocop.yml): did not find expected whitespace or line break while scanning a tag at line 2 column 7",
        ),
        (
            "Layout/LineLength:\n  X: {!b}\n",
            "({D}/.rubocop.yml): did not find expected whitespace or line break while scanning a tag at line 2 column 7",
        ),
        (
            "Layout/LineLength:\n  X: !!str#c\n",
            "({D}/.rubocop.yml): did not find expected whitespace or line break while scanning a tag at line 2 column 6",
        ),
        ("Layout/LineLength:\n  X: !!timestamp 2001-01-01\n", "Tried to load unspecified class: Date"),
        ("Layout/LineLength:\n  X: !!set {a: ~}\n", "Tried to load unspecified class: Psych::Set"),
        ("Layout/LineLength:\n  X: !ruby/range 1..2\n", "Tried to load unspecified class: Range"),
        ("Layout/LineLength:\n  X: !ruby/class String\n", "Tried to load unspecified class: String"),
        ("Layout/LineLength:\n  X: !!float x\n", "invalid value for Float(): \"x\""),
        (
            "%YAML 1\n---\nLayout/LineLength:\n  Max: 9\n",
            "({D}/.rubocop.yml): did not find expected digit or '.' character while scanning a %YAML directive at line 1 column 1",
        ),
        (
            "%YAML\n---\nLayout/LineLength:\n  Max: 9\n",
            "({D}/.rubocop.yml): did not find expected version number while scanning a %YAML directive at line 1 column 1",
        ),
        (
            "%YAML 1.123456789012\n---\nLayout/LineLength:\n  Max: 9\n",
            "({D}/.rubocop.yml): found extremely long version number while scanning a %YAML directive at line 1 column 1",
        ),
        (
            "%TAG ! \n---\nLayout/LineLength:\n  Max: 9\n",
            "({D}/.rubocop.yml): did not find expected tag URI while parsing a %TAG directive at line 1 column 1",
        ),
        (
            "%TAG !\n---\nLayout/LineLength:\n  Max: 9\n",
            "({D}/.rubocop.yml): did not find expected whitespace while scanning a %TAG directive at line 1 column 1",
        ),
        (
            "%TAG x y\n---\nLayout/LineLength:\n  Max: 9\n",
            "({D}/.rubocop.yml): did not find expected '!' while scanning a tag directive at line 1 column 1",
        ),
        (
            "Layout/LineLength:\n  X: |\n      \n    a\n",
            "({D}/.rubocop.yml): did not find expected key while parsing a block mapping at line 2 column 3",
        ),
        // #569, #570: a directive after an unmarked document needs a `---`; content left of a
        // block scalar's indentation indicator ends the (empty) scalar.
        ("a:\n  b: 1\n%YAML 1.1\n", "({D}/.rubocop.yml): did not find expected <document start> at line 1 column 1"),
        (
            "a: >2-\n false\nb: 1\n",
            "({D}/.rubocop.yml): did not find expected key while parsing a block mapping at line 1 column 1",
        ),
        (
            "AllCops:\n  SuggestExtensions: >2-\n   false\n",
            "({D}/.rubocop.yml): did not find expected key while parsing a block mapping at line 2 column 3",
        ),
    ];
    let dir = scratch("config-errors");
    fs::write(dir.join("a.rb"), "x = 1\n").unwrap();
    for (config, want) in cases {
        fs::write(dir.join(".rubocop.yml"), config).unwrap();
        let out = rbcop(&dir, &["-f", "s", "a.rb"]);
        let stderr = text(&out.stderr);
        assert_eq!(stderr.lines().next(), Some(want.replace("{D}", &dir.display().to_string()).as_str()), "{config:?}");
        assert_eq!(out.status.code(), Some(2), "{config:?}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// libyaml checks no indentation inside a flow collection or on a quoted scalar's lines (#516):
/// Psych loads each of these, yaml-rust2 0.13.0 called them "invalid indentation".
#[test]
fn flow_collections_ignore_indentation_like_libyaml() {
    let cases: &[(&str, &str)] = &[
        ("Metrics/BlockLength:\n  AllowedMethods: [\n    \"foo\",\n  ]\n", "\n  - foo\n"),
        ("Metrics/BlockLength:\n  AllowedMethods: [\n    'foo',\n ]\n", "\n  - foo\n"),
        ("Metrics/BlockLength:\n  AllowedMethods: [foo,\n  bar]\n", "\n  - foo\n  - bar\n"),
        ("Metrics/BlockLength:\n  AllowedMethods: [\n\"foo\",\nbar]\n", "\n  - foo\n  - bar\n"),
        ("Metrics/BlockLength:\n  AllowedMethods: [\"foo\n  bar\"]\n", "\n  - foo bar\n"),
        ("Metrics/BlockLength:\n  AllowedMethods: \"foo\nbar\"\n", " foo bar\n"),
        // A tab is whitespace in a flow collection and on a quoted scalar's lines (#526).
        ("Metrics/BlockLength:\n  AllowedMethods: [foo,\n\tbar]\n", "\n  - foo\n  - bar\n"),
        ("Metrics/BlockLength:\n  AllowedMethods:\n  - \"foo\n\tbar\"\n", "\n  - foo bar\n"),
        ("Metrics/BlockLength:\n  AllowedMethods: ['foo\n  \tbar', \"a\\\n\tb\"]\n", "\n  - foo bar\n  - ab\n"),
        ("Metrics/BlockLength:\n  AllowedMethods:\t[foo]\n", "\n  - foo\n"),
    ];
    let dir = scratch("flow-indent");
    fs::write(dir.join("a.rb"), "x = 1\n").unwrap();
    for (config, want) in cases {
        fs::write(dir.join(".rubocop.yml"), config).unwrap();
        let out = rbcop(&dir, &["--show-cops", "Metrics/BlockLength"]);
        assert_eq!(out.status.code(), Some(0), "{config:?}: {}", text(&out.stderr));
        assert!(text(&out.stdout).contains(&format!("  AllowedMethods:{want}")), "{config:?}: {}", text(&out.stdout));
    }
    let _ = fs::remove_dir_all(&dir);
}

/// `--show-cops` values as RuboCop 1.91 loads and dumps them (Psych 5.3.1, libyaml 0.2.5): flow
/// pairs, block scalar headers, tags, merge keys, and the emitter's styles (#561-#564, #566, #567,
/// #571, #572). Each case's text is what follows Layout/LineLength's last default.
#[test]
fn yaml_values_load_and_dump_like_psych() {
    let cases: &[(&str, &str)] = &[
        ("Layout/LineLength:\n  X: [\"a\" :]\n", "  X:\n  - a:\n"),
        ("Layout/LineLength:\n  X: [a :b]\n", "  X:\n  - a :b\n"),
        ("Layout/LineLength:\n  X: [a : b]\n", "  X:\n  - a: b\n"),
        ("Layout/LineLength:\n  X: [a: [b]]\n", "  X:\n  - a:\n    - b\n"),
        ("Layout/LineLength:\n  X: [a: {b: c}]\n", "  X:\n  - a:\n      b: c\n"),
        ("Layout/LineLength:\n  X: [a: []]\n", "  X:\n  - a: []\n"),
        ("Layout/LineLength:\n  X: [a: b, c: [d]]\n", "  X:\n  - a: b\n  - c:\n    - d\n"),
        ("Layout/LineLength:\n  X: [a: !!seq [b]]\n", "  X:\n  - a:\n    - b\n"),
        ("Layout/LineLength:\n  X: [a:\n    [b]]\n", "  X:\n  - a:\n    - b\n"),
        ("Layout/LineLength:\n  X: [? a : [b]]\n", "  X:\n  - a:\n    - b\n"),
        ("Layout/LineLength:\n  X: |#c\n    a\n", "  X: 'a\n  \n    '\n"),
        ("Layout/LineLength:\n  X: >-#c\n    a\n", "  X: a\n"),
        ("Layout/LineLength:\n  X: |\n", "  X: ''\n"),
        ("Layout/LineLength:\n  X: >\n", "  X: ''\n"),
        ("Layout/LineLength:\n  X: |+\n", "  X: ''\n"),
        ("Layout/LineLength:\n  X: |\n\n", "  X: ''\n"),
        ("Layout/LineLength:\n  X: |-\n", "  X: ''\n"),
        ("Layout/LineLength:\n  X: |", "  X: ''\n"),
        ("Layout/LineLength:\n  X: \"a\\n\"\n", "  X: 'a\n  \n    '\n"),
        ("Layout/LineLength:\n  X: \"a\\nb\\n\\n\"\n", "  X: |+\n    a\n    b\n  \n  ...\n"),
        ("Layout/LineLength:\n  X: \"\\U0001F600 \\L\"\n", "  X: \"\\U0001F600 \\L\"\n"),
        ("Layout/LineLength:\n  X: \"\\N\"\n", "  X: \"\\N\"\n"),
        ("Layout/LineLength:\n  X: \"\\ufeff\"\n", "  X: \"\\uFEFF\"\n"),
        ("Layout/LineLength:\n  X: a\u{FEFF}b\n", "  X: \"a\\uFEFFb\"\n"),
        ("Layout/LineLength:\n  X: \"a\\n\\n\"\n", "  X: \"a\\n\\n\"\n"),
        (
            "Layout/LineLength:\n  X: {kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk: 1}\n",
            "  X:\n    ? kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk\n    : 1\n",
        ),
        ("Layout/LineLength:\n  X: [!b,c]\n", "  X:\n  -\n  - c\n"),
        ("Layout/LineLength:\n  <<: [1]\n", "  !!str '<<':\n  - 1\n"),
        ("Layout/LineLength:\n  <<: [{Max: 7}, 1]\n", "  !!str '<<':\n  - Max: 7\n  - 1\n"),
        ("Layout/LineLength:\n  !!str <<: {Max: 7}\n", "  !!str '<<':\n    Max: 7\n"),
        ("Layout/LineLength:\n  <<: 1\n", "  !!str '<<': 1\n"),
        ("Layout/LineLength:\n  X: !ruby/sym a\n", "  X: :a\n"),
        ("Layout/LineLength:\n  X: !!binary aGVsbG8=\n", "  X: hello\n"),
        ("Layout/LineLength:\n  X: !!float \"1\"\n", "  X: 1.0\n"),
        ("Layout/LineLength:\n  X: !!int \"0x10\"\n", "  X: 16\n"),
        ("Layout/LineLength:\n  X: !!bool \"yes\"\n", "  X: true\n"),
        ("Layout/LineLength:\n  X: !!null \"\"\n", "  X:\n"),
        ("Layout/LineLength:\n  X: [a]#c\n", "  X:\n  - a\n"),
        ("Layout/LineLength:\n  X: {a: b}#c\n", "  X:\n    a: b\n"),
        ("Layout/LineLength:\n  X: [a,#c\n    b]\n", "  X:\n  - a\n  - b\n"),
        ("Layout/LineLength:\n  X: !ruby/regexp /a\\/b/i\n", "  X: !ruby/regexp /a\\/b/i\n"),
        ("Layout/LineLength:\n  X: :\"a b\"\n", "  X: :a b\n"),
        ("Layout/LineLength:\n  X: !ruby/sym ''\n", "  X: !ruby/symbol\n"),
    ];
    let dir = scratch("yaml-values");
    fs::write(dir.join("a.rb"), "x = 1\n").unwrap();
    for (config, want) in cases {
        fs::write(dir.join(".rubocop.yml"), config).unwrap();
        let out = rbcop(&dir, &["--show-cops", "Layout/LineLength"]);
        assert_eq!(out.status.code(), Some(0), "{config:?}: {}", text(&out.stderr));
        let stdout = text(&out.stdout);
        let tail = stdout.split_once("  SplitStrings: false\n").map(|(_, t)| t.trim_end_matches('\n'));
        assert_eq!(tail, Some(want.trim_end_matches('\n')), "{config:?}");
    }
    // A BOM at the start of the stream (or of a line) is skipped (#560); `...` ends the document.
    for config in [
        "\u{FEFF}# c\nAllCops:\n  SuggestExtensions: false\nLayout/LineLength:\n  Max: 9\n",
        "\u{FEFF}\nLayout/LineLength:\n  Max: 9\n",
        "\u{FEFF}Layout/LineLength:\n  Max: 9\n",
        "Layout/LineLength:\n  Max: 9\n... # c\n",
        "%YAML 1.1#c\n---\nLayout/LineLength:\n  Max: 9\n",
        "Layout/LineLength:\n  Max: 9\n%YAML 1.1\n---\nb: 1\n",
        "Layout/LineLength:\n  Max: 9\n%TAG ! x\n---\nb: 1\n",
    ] {
        fs::write(dir.join(".rubocop.yml"), config).unwrap();
        let out = rbcop(&dir, &["--show-cops", "Layout/LineLength"]);
        assert_eq!(out.status.code(), Some(0), "{config:?}: {}", text(&out.stderr));
        assert!(text(&out.stdout).contains("\n  Max: 9\n"), "{config:?}");
        assert!(!text(&out.stderr).contains("namespace"), "{config:?}: {}", text(&out.stderr));
    }
    let _ = fs::remove_dir_all(&dir);
}

/// An AmbiguousCopName abort inside rubocop-rspec's TopLevelGroup cops (on_new_investigation in
/// RuboCop) is a cop error without a location (#511); other RSpec cops keep the node's.
#[test]
fn top_level_group_cop_errors_have_no_location() {
    let dir = scratch("top-level-group");
    fs::write(
        dir.join(".rubocop.yml"),
        "plugins:\n  - rubocop-rspec\nAllCops:\n  NewCops: disable\n  SuggestExtensions: false\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("spec")).unwrap();
    let spec = "# rubocop:disable SelfAssignment\ndescribe \"foo\" do\n  it { expect(1).to eq 1 }\nend\ndescribe Bar do\nend\n";
    fs::write(dir.join("spec/e_spec.rb"), spec).unwrap();
    let out = rbcop(&dir, &["spec/e_spec.rb"]);
    let file = dir.join("spec/e_spec.rb").display().to_string();
    let errors: Vec<String> = text(&out.stderr)
        .lines()
        .filter_map(|l| l.strip_prefix("An error occurred while "))
        .map(|l| l.replace(&file, "F"))
        .collect();
    for want in ["RSpec/DescribeClass cop was inspecting F.", "RSpec/MultipleDescribes cop was inspecting F."] {
        assert!(errors.iter().any(|e| e == want), "{errors:?}");
    }
    assert!(errors.iter().any(|e| e == "RSpec/EmptyExampleGroup cop was inspecting F:5:0."), "{errors:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// `require:` registers a local cop file at its place, between gems too (#579): a gem with a
/// config before it memoizes the unqualified cop names first (no `no department given` warning
/// for a name only the later cops share), and the AmbiguousCopName candidates come in
/// registration order. Style/DirectiveScope's cop error there has no location (#578).
#[test]
fn local_require_between_gems_registers_in_place() {
    let dir = scratch("local-require-between-gems");
    fs::create_dir_all(dir.join("lib")).unwrap();
    fs::write(
        dir.join("lib/my_cop.rb"),
        "module RuboCop\n  module Cop\n    module Custom\n      class Casecmp < Base\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(
        dir.join(".rubocop.yml"),
        "require: [rubocop-rspec, ./lib/my_cop.rb, rubocop-performance]\nAllCops:\n  NewCops: enable\n  SuggestExtensions: false\n",
    )
    .unwrap();
    fs::write(
        dir.join("a.rb"),
        "# frozen_string_literal: true\n\n# rubocop:disable Casecmp\na = \"a\".downcase == \"b\"\n# rubocop:enable Casecmp\n",
    )
    .unwrap();
    let out = rbcop(&dir, &["-f", "e", "a.rb"]);
    let stderr = text(&out.stderr).replace(&format!("{}/", dir.display()), "");
    assert_eq!(out.status.code(), Some(2));
    assert!(!stderr.contains("no department given"), "{stderr}");
    assert!(stderr.contains("An error occurred while Style/DirectiveScope cop was inspecting a.rb.\n"), "{stderr}");
    assert!(
        stderr.ends_with(
            "Error: Ambiguous cop name `Casecmp` used in a.rb needs department qualifier. Did you mean Custom/Casecmp \
             or Performance/Casecmp?\n"
        ),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Migration/DepartmentName qualifies a bare name by the legacy cop names of RuboCop and of the
/// loaded plugins (rubocop-rspec's obsoletion rules, #583).
#[test]
fn department_name_uses_plugin_legacy_names() {
    let dir = scratch("department-name-legacy");
    fs::write(
        dir.join(".rubocop.yml"),
        "plugins: [rubocop-rspec]\nAllCops:\n  NewCops: disable\n  SuggestExtensions: false\n",
    )
    .unwrap();
    for (name, fixed) in [("FactoryBot", "RSpec/FactoryBot/*"), ("Capybara", "RSpec/Capybara/FeatureMethods")] {
        let file = dir.join("a.rb");
        fs::write(&file, format!("x = 1 # rubocop:disable Layout/ClassStructure, {name}\np x\n")).unwrap();
        let out = rbcop(&dir, &["-f", "e", "--only", "Migration/DepartmentName", "a.rb"]);
        assert!(
            text(&out.stdout).contains(":1:48: C: [Correctable] Migration/DepartmentName: Department name is missing."),
            "{}",
            text(&out.stdout)
        );
        rbcop(&dir, &["-A", "--only", "Migration/DepartmentName", "a.rb"]);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            format!("x = 1 # rubocop:disable Layout/ClassStructure, {fixed}\np x\n")
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// An enable directive opts a cop in past its target Ruby range too (Team#roundup_relevant_cops,
/// #538): Security/YAMLLoad (maximum 3.0) runs at 3.3 in a file that enables it, and a
/// disable/enable pair around its offense is not redundant.
#[test]
fn enable_directive_opts_in_outside_target_ruby() {
    let dir = scratch("enable-target-ruby");
    fs::write(
        dir.join(".rubocop.yml"),
        "AllCops:\n  TargetRubyVersion: 3.3\n  SuggestExtensions: false\n  NewCops: enable\n",
    )
    .unwrap();
    fs::write(
        dir.join("c.rb"),
        "# frozen_string_literal: true\n\n# rubocop:enable Security/YAMLLoad\np YAML.load(x)\n",
    )
    .unwrap();
    fs::write(
        dir.join("d.rb"),
        "# frozen_string_literal: true\n\nbegin\n  # rubocop:disable Security/YAMLLoad\n  p YAML.load(x)\n  \
         # rubocop:enable Security/YAMLLoad\nrescue StandardError\n  nil\nend\n",
    )
    .unwrap();
    let out = rbcop(&dir, &["-f", "e", "c.rb", "d.rb"]);
    let d = dir.display().to_string();
    assert_eq!(
        text(&out.stdout).replace(&d, "D"),
        "D/c.rb:3:18: W: [Correctable] Lint/RedundantCopEnableDirective: Unnecessary enabling of Security/YAMLLoad.\n\
         D/c.rb:4:8: W: [Correctable] Security/YAMLLoad: Prefer using `YAML.safe_load` over `YAML.load`.\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// A `RSpec: Language` list set to `~` is nil to rubocop-rspec's Language helpers, which read it
/// bare: each cop that asks it raises (#529).
#[test]
fn rspec_language_list_set_to_nil_raises() {
    let dir = scratch("rspec-language-nil");
    let config = "plugins:\n  - rubocop-rspec\nAllCops:\n  NewCops: enable\n  SuggestExtensions: false\n\
                  RSpec:\n  Language:\n    Includes:\n      Examples: ~\n";
    fs::write(dir.join(".rubocop.yml"), config).unwrap();
    fs::create_dir_all(dir.join("spec")).unwrap();
    fs::write(
        dir.join("spec/foo_spec.rb"),
        "# frozen_string_literal: true\n\ndescribe Foo do\n  it_behaves_like \"x\"\nend\n",
    )
    .unwrap();
    let out = rbcop(&dir, &["--only", "RSpec", "spec/foo_spec.rb"]);
    let file = dir.join("spec/foo_spec.rb").display().to_string();
    let printed: Vec<String> = text(&out.stderr)
        .lines()
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.strip_prefix("An error occurred while "))
        .map(|l| l.replace(&file, "F"))
        .collect();
    assert_eq!(
        printed,
        [
            "RSpec/EmptyExampleGroup cop was inspecting F:3:0.",
            "RSpec/SharedExamples cop was inspecting F:3:0.",
            "RSpec/SharedExamples cop was inspecting F:4:2."
        ]
    );
    assert!(text(&out.stdout).contains("no offenses detected"), "{}", text(&out.stdout));
    let _ = fs::remove_dir_all(&dir);
}

/// The nil list is asked where RuboCop asks it (#575, #576, #577): ancestors nearest first, the
/// arity before the list, each on_block and on_send raising on its own, and the offenses found
/// before a raise kept.
#[test]
fn rspec_language_nil_list_asked_in_rubocop_order() {
    let dir = scratch("rspec-language-nil-order");
    fs::create_dir_all(dir.join("spec")).unwrap();
    let file = dir.join("spec/foo_spec.rb").display().to_string();
    type Case<'a> = (&'a str, &'a str, &'a str, &'a [&'a str], &'a [&'a str]);
    let cases: [Case; 18] = [
        (
            "Examples:\n      Focused: ~",
            "RSpec/VoidExpect",
            "describe Foo do\n  it { expect(1) }\nend\n",
            &[
                "F:4:8: C: RSpec/VoidExpect: Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.",
            ],
            &[],
        ),
        (
            "Hooks: ~",
            "RSpec/HookArgument",
            "RSpec.describe Foo do\n  it \"a\", :focus do\n    expect(1).to eq 1\n  end\nend\n",
            &[],
            &["F:3:0."],
        ),
        (
            "Examples:\n      Regular: ~",
            "RSpec/VoidExpect",
            "RSpec.describe Foo do\n  it \"a\" do\n    expect { a }\n  end\nend\n",
            &[],
            &["F:5:4.", "F:5:4."],
        ),
        (
            "Examples:\n      Regular: ~",
            "RSpec/LeakyLocalVariable",
            "x = 1\nRSpec.describe Foo do\n  before { x }\n  it { expect(x) }\nend\n",
            &[
                "F:3:1: C: RSpec/LeakyLocalVariable: Do not use local variables defined outside of examples inside of them.",
            ],
            &["F."],
        ),
        (
            "ExampleGroups:\n      Skipped: ~",
            "RSpec/RepeatedSubjectCall",
            "RSpec.describe Foo do\n  it \"a\" do\n    expect(1).to eq 1\n  end\nend\n",
            &[],
            &[],
        ),
        // the example scope is looked up before the variable name (#601)
        (
            "ExampleGroups: ~",
            "RSpec/ExpectOutput",
            "describe Foo do\n  before do\n    $a = 1\n  end\nend\n",
            &[],
            &["F:5:4."],
        ),
        // a single-statement group body's child nodes: an assignment's value, a def's body (#602)
        (
            "Helpers: ~",
            "RSpec/ScatteredLet",
            "describe Foo do\n  context \"x\" do\n    x = foo(1, 2)\n  end\nend\n",
            &[],
            &["F:4:2."],
        ),
        (
            "Helpers: ~",
            "RSpec/IndexedLet",
            "describe Foo do\n  context \"x\" do\n    def matcher(min, max)\n      be_between(min, max)\n    end\n  end\nend\n",
            &[],
            &["F:4:2."],
        ),
        (
            "Helpers: ~",
            "RSpec/OverwritingSetup",
            "describe Foo do\n  it do\n    group = RSpec.describe do\n      ex = example(\"x\") { }\n    end\n  end\nend\n",
            &[],
            &["F:5:12."],
        ),
        // an index op-assign's send gets on_send (#603); an attribute op-assign's receiver is chained (#607)
        (
            "ExampleGroups: ~",
            "RSpec/VariableDefinition",
            "describe Foo do\n  it do\n    @h[k] += 1\n  end\nend\n",
            &[],
            &["F:3:0.", "F:4:2.", "F:5:4.", "F:5:7."],
        ),
        (
            "Examples: ~",
            "RSpec/Focus",
            "describe Foo do\n  it do\n    a.b ||= 1\n  end\nend\n",
            &[],
            &["F:3:0.", "F:4:2."],
        ),
        // a block on `super` is a parser block (#606)
        (
            "Examples: ~",
            "RSpec/EmptyExampleGroup",
            "describe Foo do\n  super do\n    it { }\n  end\nend\n",
            &[],
            &["F:3:0.", "F:4:2.", "F:5:4."],
        ),
        (
            "Expectations: ~",
            "RSpec/EmptyExampleGroup",
            "describe Foo do\n  super do\n    it { }\n  end\nend\n",
            &[],
            &[],
        ),
        (
            "ExampleGroups: ~",
            "RSpecRails/AvoidSetupHook",
            "describe Foo do\n  it do\n    super do\n    end\n  end\nend\n",
            &[],
            &["F:3:0.", "F:4:2.", "F:5:4."],
        ),
        // an `if` body's statements are a `begin`, no child of the `if`; a hook pattern checks
        // its arity before the list
        ("Helpers: ~", "RSpec/ScatteredLet", "describe Foo do\n  if c\n    foo(1, 2)\n    bar\n  end\nend\n", &[], &[]),
        (
            "Hooks: ~",
            "RSpec/SortMetadata",
            "RSpec.configure do |c|\n  c.on_example_group_definition do |g|\n    g\n  end\nend\n",
            &[],
            &[],
        ),
        // a group's own send is its first child: let? asks Helpers of a two-argument one; a
        // namespaced casgn is asked inside_describe_block?
        ("Helpers: ~", "RSpec/LetSetup", "describe Foo, :meta do\nend\n", &[], &["F:3:0."]),
        ("ExampleGroups: ~", "RSpec/LeakyConstantDeclaration", "describe Foo do\n  ::A = 1\nend\n", &[], &["F:4:2."]),
    ];
    for (list, cop, source, offenses, errors) in cases {
        let config = format!(
            "plugins:\n  - rubocop-rspec\n  - rubocop-rspec_rails\nAllCops:\n  NewCops: enable\n  SuggestExtensions: false\nRSpec:\n  Language:\n    {list}\n"
        );
        fs::write(dir.join(".rubocop.yml"), config).unwrap();
        fs::write(dir.join("spec/foo_spec.rb"), format!("# frozen_string_literal: true\n\n{source}")).unwrap();
        let out = rbcop(&dir, &["-f", "e", "--only", cop, "spec/foo_spec.rb"]);
        let stdout = text(&out.stdout).replace(&file, "F");
        assert_eq!(stdout.lines().collect::<Vec<_>>(), offenses, "{cop} {list}");
        let printed: Vec<String> = text(&out.stderr)
            .lines()
            .take_while(|l| !l.is_empty())
            .filter_map(|l| {
                l.strip_prefix(&format!("An error occurred while {cop} cop was inspecting "))
                    .map(|l| l.replace(&file, "F"))
            })
            .collect();
        assert_eq!(printed, errors, "{cop} {list}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A numeric setting given as a String or Symbol raises where the cop compares it (#610): the
/// error at the node (a block's at its send), or at the file for an investigation hook.
#[test]
fn string_numeric_setting_raises_where_compared() {
    let dir = scratch("string-numeric");
    let file = dir.join("a.rb").display().to_string();
    let cases: [(&str, &str, &[&str]); 3] = [
        ("Layout/LineLength:\n  Max: \"80\"", "x = 1\nfoo(1, 2)\n", &["F:2:0.", "F."]),
        ("Naming/BlockParameterName:\n  MinNameLength: \"2\"", "foo do |a|\nend\n", &["F:1:0."]),
        ("Metrics/MethodLength:\n  Max: :\"2\"", "def a\n  1\nend\n", &["F:1:0."]),
    ];
    for (cfg, source, errors) in cases {
        let config = format!("AllCops:\n  NewCops: enable\n  SuggestExtensions: false\n{cfg}\n");
        fs::write(dir.join(".rubocop.yml"), config).unwrap();
        fs::write(dir.join("a.rb"), source).unwrap();
        let cop = cfg.split(':').next().unwrap();
        let out = rbcop(&dir, &["-f", "e", "--only", cop, "a.rb"]);
        assert_eq!(text(&out.stdout), "", "{cfg}");
        let printed: Vec<String> = text(&out.stderr)
            .lines()
            .take_while(|l| !l.is_empty())
            .filter_map(|l| {
                l.strip_prefix(&format!("An error occurred while {cop} cop was inspecting "))
                    .map(|l| l.replace(&file, "F"))
            })
            .collect();
        assert_eq!(printed, errors, "{cfg}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// RuboCop's built-in plugin is one for a String `plugins:` entry only: a Hash entry requires its
/// `require_path` (default: the name, no such file) and needs a plugin class (#597).
#[test]
fn internal_affairs_hash_entry_is_no_builtin() {
    let dir = scratch("internal-affairs-hash");
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nx = 1\np x\n").unwrap();
    let cases = [
        ("{}", 2, "cannot load such file -- rubocop-internal_affairs"),
        (
            "{plugin_class_name: RuboCop::InternalAffairs::Plugin}",
            2,
            "cannot load such file -- rubocop-internal_affairs",
        ),
        (
            "{require_path: rubocop/cop/internal_affairs}",
            2,
            "Error: Failed to load plugin `rubocop-internal_affairs` because the corresponding plugin class could not be \
             determined for instantiation.",
        ),
        ("{enabled: false}", 0, ""),
    ];
    for (entry, status, first) in cases {
        let config = format!(
            "plugins:\n  - rubocop-internal_affairs: {entry}\nAllCops:\n  NewCops: disable\n  SuggestExtensions: false\n"
        );
        fs::write(dir.join(".rubocop.yml"), config).unwrap();
        let out = rbcop(&dir, &["-f", "e", "a.rb"]);
        assert_eq!(out.status.code(), Some(status), "{entry}");
        assert_eq!(text(&out.stderr).lines().next().unwrap_or(""), first, "{entry}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Lint/RedundantCopDisableDirective's did-you-mean list is the registry of the first file that
/// described an unknown cop name (SIMILAR_COP_NAMES_CACHE, #599): a plugin a later subdirectory
/// config loads is not in it.
#[test]
fn similar_cop_names_frozen_at_first_unknown_name() {
    let dir = scratch("similar-cop-names");
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  SuggestExtensions: false\n").unwrap();
    fs::write(dir.join("sub/.rubocop.yml"), "plugins: [rubocop-rspec]\n").unwrap();
    fs::write(
        dir.join("sub/a.rb"),
        "# frozen_string_literal: true\n\nx = 1 # rubocop:disable RSpec/Capybara/FeatureMethods\np x\n",
    )
    .unwrap();
    let redundant = |out: &Output| -> Vec<String> {
        text(&out.stdout)
            .lines()
            .filter(|l| l.contains("Redundant"))
            .map(|l| l.split(": W: ").nth(1).unwrap().to_string())
            .collect()
    };
    for (b, last) in [
        ("x = 1 # rubocop:disable Style/Foo\np x\n", "(unknown cop)"),
        ("x = 1\np x\n", "(did you mean `RSpec/DescribeMethod`?)"),
    ] {
        fs::write(dir.join("b.rb"), b).unwrap();
        let out = rbcop(&dir, &["-f", "e"]);
        let lines = redundant(&out);
        assert!(
            lines.last().is_some_and(|l| l.ends_with(&format!("`RSpec/Capybara/FeatureMethods` {last}."))),
            "{lines:?}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// RSpec/DiscardedMatcher with a String CustomMatcherMethods (#502): every receiverless send
/// raises, and a plain block's on_block raises again; a numblock call raises once.
#[test]
fn discarded_matcher_string_setting_raises_per_callback() {
    let dir = scratch("discarded-matcher");
    let config = "plugins:\n  - rubocop-rspec\nAllCops:\n  NewCops: disable\n  SuggestExtensions: false\n\
                  RSpec/DiscardedMatcher:\n  CustomMatcherMethods: foo\n";
    fs::write(dir.join(".rubocop.yml"), config).unwrap();
    fs::create_dir_all(dir.join("spec")).unwrap();
    fs::write(dir.join("spec/a_spec.rb"), "describe \"x\" do\nend\nbar { _1 }\nx.y { 1 }\n").unwrap();
    let out = rbcop(&dir, &["--only", "RSpec/DiscardedMatcher"]);
    let file = dir.join("spec/a_spec.rb").display().to_string();
    let stderr = text(&out.stderr);
    let printed: Vec<String> = stderr
        .lines()
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.strip_prefix("An error occurred while RSpec/DiscardedMatcher cop was inspecting "))
        .map(|l| l.replace(&file, "F"))
        .collect();
    assert_eq!(printed, ["F:1:0.", "F:1:0.", "F:3:0.", "F:4:0."]);
    let _ = fs::remove_dir_all(&dir);
}

/// DisplayStyleGuide: the first offense with a StyleGuide anchor fixes its URL for the run, as
/// RuboCop's per-process MessageAnnotator.style_guide_urls does (#508).
#[test]
fn style_guide_urls_are_resolved_once_per_anchor() {
    let dir = scratch("style-guide-cache");
    let config = "plugins: [rubocop-rspec]\nAllCops:\n  SuggestExtensions: false\n  DisabledByDefault: true\n  \
                  DisplayStyleGuide: true\nStyle/ClassCheck:\n  Enabled: true\nRSpec/ClassCheck:\n  Enabled: true\n";
    fs::write(dir.join(".rubocop.yml"), config).unwrap();
    fs::write(dir.join("a.rb"), "x.kind_of?(Y)\n").unwrap();
    fs::create_dir_all(dir.join("spec")).unwrap();
    fs::write(dir.join("spec/b_spec.rb"), "describe Foo do\n  it { expect(x).to be_a_kind_of(Y) }\nend\n").unwrap();
    for (first, base) in [("a.rb", "https://rubystyle.guide"), ("spec/b_spec.rb", "https://rspec.rubystyle.guide")] {
        let other = if first == "a.rb" { "spec/b_spec.rb" } else { "a.rb" };
        let out = text(&rbcop(&dir, &["-f", "e", first, other]).stdout);
        let n = out.lines().filter(|l| l.contains(&format!("({base}#is-a-vs-kind-of"))).count();
        assert_eq!(n, 2, "{out}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The error summary lists every raise, two at one location too (#501); under -a/-A only the last
/// (non-correcting) pass runs the cops that do not correct, so their errors are not repeated.
#[test]
fn cop_error_summary_keeps_duplicates() {
    let dir = scratch("error-dups");
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: disable\nLint/Debugger:\n  DebuggerMethods: x\n")
        .unwrap();
    let file = dir.join("a.rb");
    for (args, lines) in [(&[][..], [1, 1, 3]), (&["-a"][..], [1, 1, 3]), (&["-A"][..], [3, 3, 5])] {
        fs::write(&file, "binding.pry\nx = \"a\"\nputs  x\n").unwrap();
        let out = rbcop(&dir, &[args, &["a.rb"]].concat());
        let stderr = text(&out.stderr);
        let summary = stderr.split_once("3 errors occurred:\n").map(|s| s.1).unwrap_or_else(|| panic!("{stderr}"));
        let want: Vec<String> = lines
            .iter()
            .map(|l| format!("An error occurred while Lint/Debugger cop was inspecting {}:{l}:0.", file.display()))
            .collect();
        assert_eq!(summary.lines().take(3).collect::<Vec<_>>(), want, "{args:?}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Cop errors printed during the run come in target order, whichever worker finishes first (#507).
#[test]
fn cop_errors_print_in_target_order() {
    let dir = scratch("error-order");
    fs::write(
        dir.join(".rubocop.yml"),
        "AllCops:\n  NewCops: disable\n  SuggestExtensions: false\nNaming/FileName:\n  ExpectMatchingDefinition: true\n  AllowedAcronyms: CLI\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("lib")).unwrap();
    let mut want = Vec::new();
    for i in 0..16 {
        let name = format!("lib/f{i:02}.rb");
        // Bigger files first in the worker queue: the last target is the largest.
        let body = format!("# frozen_string_literal: true\n\n{}class Other; end\n", "x = 1\n".repeat(i * 200));
        fs::write(dir.join(&name), body).unwrap();
        want.push(format!("An error occurred while Naming/FileName cop was inspecting {}.", dir.join(&name).display()));
    }
    for _ in 0..3 {
        let out = rbcop(&dir, &["--only", "Naming/FileName"]);
        let stderr = text(&out.stderr);
        let printed: Vec<&str> =
            stderr.lines().take_while(|l| !l.is_empty()).filter(|l| l.starts_with("An error")).collect();
        assert_eq!(printed, want);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A corrector that clobbers (Style/IfInsideElse on a one-line nested if) is a cop error located
/// at the node the cop was visiting (the outer `if`, 1:0), not at the offense (the inner `if`).
#[test]
fn cop_error_is_located_at_the_visited_node() {
    let dir = scratch("coperror");
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: disable\n  TargetRubyVersion: 2.7\n").unwrap();
    fs::write(dir.join("t.rb"), "if cond then foo else if cond2; bar else baz end; end\n").unwrap();
    let out = rbcop(&dir, &["--only", "Style/IfInsideElse", "-f", "e", "t.rb"]);
    let err = text(&out.stderr);
    // Team#process_errors names the file by its absolute path (Dir.pwd is physical).
    let abs = dir.join("t.rb");
    let want = format!("An error occurred while Style/IfInsideElse cop was inspecting {}:1:0.\n", abs.display());
    assert!(err.starts_with(&want), "{err}");
    assert_eq!(out.status.code(), Some(1));
    let _ = fs::remove_dir_all(&dir);
}

/// InfiniteCorrectionLoop lists each iteration's cops in team order, which Registry#sort! makes
/// cop-name order (specs corpus style/redundant_line_continuation/089.rb).
#[test]
fn infinite_loop_names_cops_in_name_order() {
    let dir = scratch("infloop");
    fs::write(dir.join("t.rb"), "class Foo\n  memoize \\\n  def do_something\n  end\nend\n").unwrap();
    let only = "Layout/DefEndAlignment,Layout/TrailingWhitespace,Style/EmptyMethod,Style/Semicolon";
    let out = rbcop(&dir, &["-A", "--only", only, "--force-default-config", "-f", "e", "t.rb"]);
    let want = format!(
        "Infinite loop detected in {} and caused by Layout/DefEndAlignment -> Layout/TrailingWhitespace, \
         Style/EmptyMethod, Style/Semicolon\n",
        dir.join("t.rb").display()
    );
    assert!(text(&out.stderr).contains(&want), "{}", text(&out.stderr));
    assert_eq!(out.status.code(), Some(1));
    let _ = fs::remove_dir_all(&dir);
}

/// Option values RuboCop's cops raise on (#114): MinBodyLength / MinBranchesCount not a positive
/// Integer, SafeNavigationChainLength `Max: 0` (add_offense(nil), also for a csend with a block),
/// a zero IndentationWidth with tabs (ZeroDivisionError). Each is a cop error at the visited node
/// (IndentationStyle's in on_new_investigation, so the bare path) and the offense is lost.
/// Expected lines recorded from RuboCop 1.91.
#[test]
fn config_values_cops_raise_on_are_cop_errors() {
    let cases = [
        (
            "Style/GuardClause:\n  MinBodyLength: 0\nStyle/Next:\n  MinBodyLength: 1.5\nStyle/CaseLikeIf:\n  \
             MinBranchesCount: 0\nStyle/HashLikeCase:\n  MinBranchesCount: -1\nStyle/SafeNavigationChainLength:\n  Max: 0\n",
            "def m\n  if a\n    b\n  end\nend\n[1].each do |x|\n  if x\n    y\n  end\nend\nif x == 1\n  a\nelsif x == 2\n  \
             b\nend\ncase x\nwhen 1 then :a\nwhen 2 then :b\nend\nfoo&.bar\nfoo&.bar { }\n",
            "Style/GuardClause,Style/Next,Style/CaseLikeIf,Style/HashLikeCase,Style/SafeNavigationChainLength",
            &[
                "Style/GuardClause:1:0",
                "Style/Next:6:0",
                "Style/CaseLikeIf:11:0",
                "Style/HashLikeCase:16:0",
                "Style/SafeNavigationChainLength:20:0",
                "Style/SafeNavigationChainLength:21:0",
            ][..],
        ),
        (
            "Layout/IndentationWidth:\n  Width: 0\nLayout/IndentationStyle:\n  EnforcedStyle: tabs\n",
            "def m\n  x\nend\n",
            "Layout/IndentationWidth,Layout/IndentationStyle",
            &["Layout/IndentationStyle", "Layout/IndentationWidth:1:0"][..],
        ),
    ];
    for (config, source, only, want) in cases {
        let dir = scratch("configraise");
        fs::write(
            dir.join(".rubocop.yml"),
            format!("AllCops:\n  NewCops: enable\n  SuggestExtensions: false\n{config}"),
        )
        .unwrap();
        fs::write(dir.join("a.rb"), source).unwrap();
        let out = rbcop(&dir, &["--only", only, "-f", "e", "a.rb"]);
        let abs = dir.join("a.rb").display().to_string();
        let want: Vec<String> = want
            .iter()
            .map(|w| match w.split_once(':') {
                Some((cop, at)) => format!("An error occurred while {cop} cop was inspecting {abs}:{at}."),
                None => format!("An error occurred while {w} cop was inspecting {abs}."),
            })
            .collect();
        let err = text(&out.stderr);
        // The end-of-run summary, as a set: rbcop sorts it (runner::take_errors) where RuboCop
        // keeps raise order, and runs on_file cops after the walk, so the errors printed as they
        // happen put IndentationStyle's (RuboCop: on_new_investigation, first) last.
        let mut want = want;
        want.sort();
        let mut got: Vec<&str> = err
            .lines()
            .skip_while(|l| !l.ends_with("errors occurred:"))
            .skip(1)
            .take_while(|l| l.starts_with("An error"))
            .collect();
        got.sort();
        assert_eq!(got, want, "{err}");
        assert_eq!(text(&out.stdout), "", "{only}");
        assert_eq!(out.status.code(), Some(1));
        let _ = fs::remove_dir_all(&dir);
    }
}

/// The processed file name is the expanded absolute path: `__FILE__`'s value (Style/StringConcatenation
/// folds it into the literal) and a cop Warning's `(from file: ...)`, which ExecuteRunner repeats
/// in its end-of-run `N warnings:` summary, one per raise (two -A iterations here).
#[test]
fn file_name_is_the_absolute_path() {
    let dir = scratch("abspath");
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("sub/t.rb"), "x = __FILE__ + 'top'\n").unwrap();
    let only = "Style/StringConcatenation,Style/Copyright";
    let out = rbcop(&dir, &["-A", "--only", only, "--force-default-config", "-f", "e", "sub/../sub/t.rb"]);
    let abs = dir.join("sub/t.rb").display().to_string();
    assert_eq!(fs::read_to_string(dir.join("sub/t.rb")).unwrap(), format!("x = \"{abs}top\"\n"));
    let warning =
        format!("Style/Copyright: An AutocorrectNotice must be defined in your RuboCop config (from file: {abs})");
    let err = text(&out.stderr);
    assert!(err.ends_with(&format!("\n2 warnings:\n{warning}\n{warning}\n")), "{err}");
    assert_eq!(err.matches(&warning).count(), 4, "{err}");
    assert_eq!(out.status.code(), Some(0));
    let _ = fs::remove_dir_all(&dir);
}

/// inherit_gem: the rubocop gem is refused (ConfigLoaderResolver's ArgumentError), and a gem that
/// is not installed gets gem_config_path's `Unable to find gem ...` with RubyGems' message.
#[test]
fn inherit_gem_errors_worded_like_rubocop() {
    let dir = scratch("inherit-gem");
    fs::write(dir.join("a.rb"), "x = 1\n").unwrap();
    fs::write(dir.join(".rubocop.yml"), "inherit_gem:\n  rubocop: config/default.yml\n").unwrap();
    let out = rbcop(&dir, &["-f", "s", "a.rb"]);
    assert_eq!(text(&out.stderr), "can't inherit configuration from the rubocop gem\n");
    assert_eq!(out.status.code(), Some(2));
    fs::write(dir.join(".rubocop.yml"), "inherit_gem:\n  nonexistent_gem_xyz: x.yml\n").unwrap();
    let out = rbcop(&dir, &["-f", "s", "a.rb"]);
    let stderr = text(&out.stderr);
    // Without a ruby on PATH there is no RubyGems message to compare.
    if !stderr.contains("cannot run ruby") {
        assert!(
            stderr.starts_with(
                "Unable to find gem nonexistent_gem_xyz; is the gem installed? Could not find 'nonexistent_gem_xyz' (>= 0)"
            ),
            "{stderr}"
        );
    }
    assert_eq!(out.status.code(), Some(2));
    let _ = fs::remove_dir_all(&dir);
}

/// --init (InitDotfile), recorded with RuboCop 1.91: writes the commented template and says where;
/// a second run finds the file and fails (exit 2).
#[test]
fn init_writes_rubocop_yml_once() {
    let dir = scratch("init");
    fs::remove_file(dir.join(".rubocop.yml")).unwrap();
    let out = rbcop(&dir, &["--init"]);
    let path = dir.join(".rubocop.yml").display().to_string();
    assert_eq!(text(&out.stdout), format!("Writing new .rubocop.yml to {path}\n"));
    assert_eq!(out.status.code(), Some(0));
    let written = fs::read_to_string(dir.join(".rubocop.yml")).unwrap();
    assert!(written.starts_with("# The behavior of RuboCop can be controlled via the .rubocop.yml\n"));
    assert!(written.ends_with("#\n# See https://docs.rubocop.org/rubocop/configuration\n"));
    assert_eq!(written.lines().count(), 10);
    let out = rbcop(&dir, &["--init"]);
    assert_eq!(text(&out.stderr), format!(".rubocop.yml already exists at {path}\n"));
    assert_eq!(out.status.code(), Some(2));
    let _ = fs::remove_dir_all(&dir);
}

/// -h under colour (CLICOLOR_FORCE=1 rubocop -h): the banner and section headings are bright,
/// unless --no-color is on the command line.
#[test]
fn help_headings_are_bright_when_coloured() {
    let dir = scratch("help");
    let run = |args: &[&str]| {
        common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(args)
            .env("CLICOLOR_FORCE", "1")
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let out = text(&run(&["-h"]).stdout);
    assert!(
        out.starts_with("\x1b[1mUsage: rubocop [options] [file1, file2, ...]\x1b[0m\n\n\x1b[1mBasic Options\x1b[0m:\n")
    );
    assert_eq!(out.matches("\x1b[").count(), 24);
    assert!(!text(&run(&["-h", "--no-color"]).stdout).contains('\x1b'));
    let _ = fs::remove_dir_all(&dir);
}

/// Lint/SelfAssignment with AllowRBSInlineAnnotation associates comments once per file, not once
/// per offense (20,000 offenses took 36 s of CPU when each one walked the whole file; 40,000
/// quadruple that, so a loaded machine still separates the two).
#[test]
fn self_assignment_rbs_annotation_is_linear() {
    let dir = scratch("self-assignment-rbs");
    fs::write(
        dir.join(".rubocop.yml"),
        "AllCops:\n  NewCops: disable\nLint/SelfAssignment:\n  AllowRBSInlineAnnotation: true\n",
    )
    .unwrap();
    fs::write(dir.join("a.rb"), format!("a = 1\n{}", "a = a\n".repeat(40_000))).unwrap();
    let t0 = std::time::Instant::now();
    let out = rbcop(&dir, &["--only", "Lint/SelfAssignment", "-f", "q", "a.rb"]);
    assert!(t0.elapsed() < std::time::Duration::from_secs(60), "took {:?}", t0.elapsed());
    assert!(text(&out.stdout).ends_with("1 file inspected, 40000 offenses detected\n"));
    let _ = fs::remove_dir_all(&dir);
}

/// JUnit lists Lint/Syntax as RuboCop's Registry#enabled does (Config#enable_cop? is always true
/// for it), under --disable-all-cops and with the Lint department disabled.
#[test]
fn junit_lists_lint_syntax_when_cops_are_disabled() {
    let dir = scratch("junit-syntax");
    fs::write(dir.join("a.rb"), "x = 1\n").unwrap();
    let out = text(&rbcop(&dir, &["--disable-all-cops", "-f", "ju", "a.rb"]).stdout);
    assert_eq!(
        out,
        "<?xml version='1.0'?>\n<testsuites>\n  <testsuite name='rubocop' tests='1' failures='0'>\n    \
         <testcase classname='a' name='Lint/Syntax'/>\n  </testsuite>\n</testsuites>\n"
    );
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: disable\nLint:\n  Enabled: false\n").unwrap();
    let out = text(&rbcop(&dir, &["-f", "ju", "a.rb"]).stdout);
    assert!(out.contains("<testcase classname='a' name='Lint/Syntax'/>"));
    assert!(!out.contains("name='Lint/AmbiguousOperator'"));
    let _ = fs::remove_dir_all(&dir);
}

/// A config regexp with catastrophic backtracking (fancy-regex path) on a 100k-character name stays
/// within one bounded retry budget per match instead of len * 100k steps.
#[test]
fn backtracking_config_regexp_on_a_long_name_is_bounded() {
    let dir = scratch("backtracking-long");
    fs::write(
        dir.join(".rubocop.yml"),
        "AllCops:\n  NewCops: disable\nNaming/VariableName:\n  AllowedPatterns:\n    - '(a|aa)+(?=c)'\n",
    )
    .unwrap();
    fs::write(dir.join("a.rb"), format!("{}B = 1\n", "a".repeat(100_000))).unwrap();
    let t0 = std::time::Instant::now();
    let out = rbcop(&dir, &["--only", "Naming/VariableName", "-f", "q", "a.rb"]);
    assert!(t0.elapsed() < std::time::Duration::from_secs(30), "took {:?}", t0.elapsed());
    assert!(text(&out.stdout).ends_with("1 file inspected, 1 offense detected\n"), "{}", text(&out.stdout));
    let _ = fs::remove_dir_all(&dir);
}

/// The corpus gates fail loudly when the binary they test is missing or lists no cops (#163). A
/// missing binary once produced an empty cop list, filtered both sides to nothing and reported
/// `all corpora match`. `cargo` is stubbed so the scripts' rebuild cannot restore the binary.
#[test]
fn gate_scripts_fail_without_a_binary_or_cops() {
    let dir = scratch("gate-scripts");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stubs = dir.join("bin");
    fs::create_dir_all(stubs.join("release")).unwrap();
    fs::write(stubs.join("cargo"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(stubs.join("cargo"), fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap_or_default());
    let run = |script: &str, args: &[&str]| {
        let out = std::process::Command::new("sh")
            .arg(root.join("scripts").join(script))
            .args(args)
            .env("PATH", &path)
            .env("CARGO_TARGET_DIR", &stubs)
            .env("RBCOP_CORPUS", &dir)
            .env("RBCOP_ORACLE", &dir)
            .current_dir(&dir)
            .output()
            .unwrap();
        (out.status.code(), text(&out.stdout), text(&out.stderr))
    };
    // No binary at all.
    let (code, out, err) = run("check-cops.sh", &["all", "specs"]);
    assert_eq!(code, Some(1), "{out}{err}");
    assert!(err.contains("rbcop does not run as"), "{out}{err}");
    assert!(!out.contains("all corpora match"), "{out}");
    // A binary that runs but implements no cops.
    let fake = stubs.join("release/rbcop");
    fs::write(&fake, "#!/bin/sh\n[ \"$1\" = --version ] && echo 0.0.0\nexit 0\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    for (script, args) in [("check-cops.sh", &["all", "specs"][..]), ("compare.sh", &["."][..])] {
        let (code, out, err) = run(script, args);
        assert_eq!(code, Some(1), "{script}: {out}{err}");
        assert!(err.contains("no cops from"), "{script}: {out}{err}");
    }
    // An explicitly empty cop list.
    let (code, _, err) = run("check-cops.sh", &["", "specs"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("empty cop list"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

/// Deliberate difference: -a/-A never writes a file that resolves outside the project (the working
/// directory and the command-line paths). The file is reported as without -a, with one warning.
fn assert_refused(dir: &Path, outside_file: &Path, shown: &str, args: &[&str]) {
    let plain = rbcop(dir, &[SPACING, args].concat());
    for flag in ["-a", "-A"] {
        let out = rbcop(dir, &[&[flag], SPACING, args].concat());
        assert_eq!(fs::read_to_string(outside_file).unwrap(), "x  =  1\n", "{flag}");
        assert_eq!(text(&out.stdout), text(&plain.stdout), "{flag}");
        assert!(!text(&out.stdout).contains("[Corrected]"), "{flag}");
        assert_eq!(out.status.code(), Some(1), "{flag}");
        let real = outside_file.canonicalize().unwrap();
        let warning =
            format!("rbcop: not autocorrecting {shown}: it resolves to {}, outside the project\n", real.display());
        assert_eq!(text(&out.stderr), warning, "{flag}");
    }
}

/// Correcting needs only search (x) permission on the directories down to the file, as RuboCop's
/// path-based write does: a directory that cannot be listed (mode 0300), as an intermediate
/// directory, as the file's own directory, or as the working directory, is no obstacle.
#[test]
fn autocorrect_writes_through_unreadable_directories() {
    if root() {
        return;
    }
    let dir = scratch("noread");
    fs::create_dir_all(dir.join("sub/deep")).unwrap();
    for f in ["sub/a.rb", "sub/b.rb", "sub/deep/c.rb"] {
        fs::write(dir.join(f), "x  =  1\n").unwrap();
    }
    for d in ["sub/deep", "sub"] {
        fs::set_permissions(dir.join(d), fs::Permissions::from_mode(0o300)).unwrap();
    }
    let mut args = vec!["-a"];
    args.extend(SPACING);
    let mut outs = vec![];
    for (cwd, file) in [(dir.clone(), "sub/a.rb"), (dir.clone(), "sub/deep/c.rb"), (dir.join("sub"), "b.rb")] {
        let mut a = args.clone();
        a.push(file);
        outs.push(rbcop(&cwd, &a));
    }
    for d in ["sub", "sub/deep"] {
        fs::set_permissions(dir.join(d), fs::Permissions::from_mode(0o755)).unwrap();
    }
    for out in outs {
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        assert!(text(&out.stdout).contains("[Corrected]"), "{}", text(&out.stdout));
    }
    for f in ["sub/a.rb", "sub/b.rb", "sub/deep/c.rb"] {
        assert_eq!(fs::read_to_string(dir.join(f)).unwrap(), "x = 1\n", "{f}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn autocorrect_refuses_symlinked_dir_outside_project() {
    let dir = scratch("escape-dir");
    let outside = scratch("escape-dir-outside");
    fs::write(outside.join("o.rb"), "x  =  1\n").unwrap();
    symlink(&outside, dir.join("ext")).unwrap();
    assert_refused(&dir, &outside.join("o.rb"), "ext/o.rb", &[]);
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&outside);
}

#[test]
fn autocorrect_refuses_symlinked_file_outside_project() {
    let dir = scratch("escape-file");
    let outside = scratch("escape-file-outside");
    fs::write(outside.join("o.rb"), "x  =  1\n").unwrap();
    symlink(outside.join("o.rb"), dir.join("l.rb")).unwrap();
    assert_refused(&dir, &outside.join("o.rb"), "l.rb", &[]);
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&outside);
}

/// Paths given explicitly may lie outside the working directory; `..` that stays inside and a
/// symlink that resolves inside are corrected as usual.
#[test]
fn autocorrect_writes_explicit_and_inside_paths() {
    let dir = scratch("inside");
    let outside = scratch("inside-outside");
    fs::create_dir_all(outside.join("d")).unwrap();
    for f in [outside.join("o.rb"), outside.join("d/o.rb")] {
        fs::write(f, "x  =  1\n").unwrap();
    }
    fs::create_dir_all(dir.join("lib/sub")).unwrap();
    for f in ["lib/a.rb", "lib/sub/b.rb", "c.rb"] {
        fs::write(dir.join(f), "x  =  1\n").unwrap();
    }
    symlink("lib", dir.join("alias")).unwrap();
    let (o, d) = (outside.join("o.rb"), outside.join("d"));
    let mut args = vec!["-a"];
    args.extend(SPACING);
    args.extend([o.to_str().unwrap(), d.to_str().unwrap(), "alias/a.rb", "lib/sub/../../c.rb", "lib/sub/b.rb"]);
    let out = rbcop(&dir, &args);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(!text(&out.stderr).contains("not autocorrecting"), "{}", text(&out.stderr));
    for f in [o, outside.join("d/o.rb"), dir.join("lib/a.rb"), dir.join("c.rb"), dir.join("lib/sub/b.rb")] {
        assert_eq!(fs::read_to_string(&f).unwrap(), "x = 1\n", "{}", f.display());
    }
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&outside);
}

/// A directive naming an ambiguous bare cop (AmbiguousCopName) makes every add_offense raise.
/// RuboCop's VariableForce cops run as one force, so only the first hook to add an offense errors,
/// without a location (#471). Expected cops recorded from RuboCop 1.91.
#[test]
fn ambiguous_directive_errors_once_for_the_variable_force() {
    let dir = scratch("forceerror");
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: enable\n  SuggestExtensions: false\n").unwrap();
    let head = "# frozen_string_literal: true\n\n# rubocop:disable SelfAssignment\n";
    let cases = [
        ("a.rb", format!("{head}def f(a)\n  x = 1\n  [1].each {{ |b| 1 }}\nend\n"), "Lint/UnusedBlockArgument"),
        ("b.rb", "x = 1 # rubocop:disable SelfAssignment\n".to_string(), "Lint/UselessAssignment"),
        (
            "c.rb",
            format!("{head}def g(a, _b)\n  _b\nend\n\ndef h(c)\n  d = 1\n  [1].each do |d|\n    e = 2\n  end\nend\n"),
            "Lint/UnderscorePrefixedVariableName",
        ),
        ("d.rb", format!("{head}def m\n  y = 1\n  [1].each {{ |y| p y }}\n  z = 2\nend\n"), "Lint/UselessAssignment"),
    ];
    for (name, source, cop) in cases {
        fs::write(dir.join(name), source).unwrap();
        let out = rbcop(&dir, &[name]);
        let force: Vec<String> = text(&out.stderr)
            .lines()
            .filter(|l| {
                l.starts_with("An error occurred while ")
                    && ["Lint/Shadow", "Lint/Underscore", "Lint/Unused", "Lint/Useless"].iter().any(|c| l.contains(c))
            })
            .map(String::from)
            .collect();
        let want = format!("An error occurred while {cop} cop was inspecting {}.", dir.join(name).display());
        // The run ends at the file: no error summary.
        assert_eq!(force, [want], "{name}");
        assert_eq!(out.status.code(), Some(2), "{name}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// List settings given as a YAML String (#474) or set to `~` (#500): where RuboCop's cop reads them bare and then calls
/// an Array-only method (`map`, `any?`, `+`, `-`, `join`, `to_set`), the cop raises at the node it
/// visits. Cop errors recorded from RuboCop 1.91 on the two scenario projects (their offenses are
/// in expected.txt).
#[test]
fn string_list_settings_raise_where_rubocop_does() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios");
    let cases: [(&str, &[&str]); 4] = [
        (
            "list_settings_string",
            &["Lint/MissingSuper cop was inspecting a.rb:21:2.", "Style/AsciiComments cop was inspecting a.rb."],
        ),
        (
            "list_settings_string_sweep",
            &[
                "Lint/UselessAccessModifier cop was inspecting a.rb:15:2.",
                "Lint/UselessAccessModifier cop was inspecting a.rb:30:0.",
                "Lint/UselessAccessModifier cop was inspecting a.rb:37:0.",
                "Lint/UselessAccessModifier cop was inspecting a.rb:4:0.",
                "Naming/ClassAndModuleCamelCase cop was inspecting a.rb:22:0.",
                "Naming/HeredocDelimiterNaming cop was inspecting a.rb:23:4.",
                "Naming/MethodName cop was inspecting a.rb:10:2.",
                "Naming/MethodName cop was inspecting a.rb:18:4.",
                "Naming/MethodName cop was inspecting a.rb:26:0.",
                "Naming/MethodName cop was inspecting a.rb:27:0.",
                "Naming/MethodName cop was inspecting a.rb:38:2.",
                "Naming/MethodName cop was inspecting a.rb:6:2.",
                "Style/ArgumentsForwarding cop was inspecting a.rb:27:0.",
                "Style/CommentAnnotation cop was inspecting a.rb.",
                "Style/GlobalVars cop was inspecting a.rb:33:0.",
                "Style/IfUnlessModifier cop was inspecting a.rb:41:0.",
                "Style/NumericLiterals cop was inspecting a.rb:33:7.",
                "Style/NumericLiterals cop was inspecting a.rb:34:4.",
                "Style/NumericLiterals cop was inspecting a.rb:35:4.",
                "Style/NumericLiterals cop was inspecting a.rb:39:10.",
            ],
        ),
        // `__LINE__` is an int node to RuboCop (#514).
        (
            "numeric_literals_source_line",
            &[
                "Style/NumericLiterals cop was inspecting a.rb:1:2.",
                "Style/NumericLiterals cop was inspecting a.rb:2:2.",
            ],
        ),
        // List settings set to `~` (#500): the merge removes the key, so a bare read is nil.
        (
            "list_settings_nil",
            &[
                "Layout/ClassStructure cop was inspecting a.rb:15:0.",
                "Layout/ClassStructure cop was inspecting a.rb:7:0.",
                "Layout/ClassStructure cop was inspecting app/controllers/posts_controller.rb:4:0.",
                "Layout/MultilineAssignmentLayout cop was inspecting a.rb:11:0.",
                "Layout/MultilineAssignmentLayout cop was inspecting a.rb:20:0.",
                "Layout/MultilineAssignmentLayout cop was inspecting a.rb:3:0.",
                "Layout/MultilineAssignmentLayout cop was inspecting a.rb:5:0.",
                "Lint/ConstantResolution cop was inspecting a.rb:15:6.",
                "Lint/ConstantResolution cop was inspecting a.rb:4:0.",
                "Lint/ConstantResolution cop was inspecting a.rb:5:4.",
                "Lint/ConstantResolution cop was inspecting a.rb:7:6.",
                "Lint/ConstantResolution cop was inspecting app/controllers/posts_controller.rb:4:24.",
                "Lint/ConstantResolution cop was inspecting app/controllers/posts_controller.rb:4:6.",
                "Naming/ClassAndModuleCamelCase cop was inspecting a.rb:7:0.",
                "Naming/MethodParameterName cop was inspecting a.rb:8:2.",
                "Naming/PredicatePrefix cop was inspecting a.rb:14:0.",
                "Naming/PredicatePrefix cop was inspecting a.rb:18:2.",
                "Naming/PredicatePrefix cop was inspecting a.rb:23:15.",
                "Naming/PredicatePrefix cop was inspecting a.rb:24:0.",
                "Rails/ActionOrder cop was inspecting app/controllers/posts_controller.rb:4:0.",
                "Style/BlockDelimiters cop was inspecting a.rb:10:0.",
                "Style/BlockDelimiters cop was inspecting a.rb:14:0.",
                "Style/BlockDelimiters cop was inspecting a.rb:20:4.",
                "Style/BlockDelimiters cop was inspecting a.rb:23:0.",
                "Style/CommentAnnotation cop was inspecting a.rb.",
                "Style/CommentAnnotation cop was inspecting app/controllers/posts_controller.rb.",
                "Style/Documentation cop was inspecting a.rb:7:0.",
                "Style/Documentation cop was inspecting app/controllers/posts_controller.rb:4:0.",
                "Style/FetchEnvVar cop was inspecting a.rb:4:0.",
                "Style/GlobalVars cop was inspecting a.rb:3:0.",
                "Style/SingleLineBlockParams cop was inspecting a.rb:10:0.",
                "Style/SingleLineBlockParams cop was inspecting a.rb:23:0.",
            ],
        ),
    ];
    for (name, want) in cases {
        let project = root.join(name).join("project");
        let out = rbcop(&project, &["-f", "e"]);
        let dir = format!("{}/", project.canonicalize().unwrap().display());
        // Each error is printed as it happens and again in the summary.
        let mut errors: Vec<String> = text(&out.stderr)
            .lines()
            .filter_map(|l| l.strip_prefix("An error occurred while "))
            .map(|l| l.replace(&dir, ""))
            .collect();
        errors.sort();
        errors.dedup();
        assert_eq!(errors, want, "{name}");
    }
}

/// Below 3.3, whether the parser gem rejects a file where only the Lint/Syntax wording differs
/// (a documented gap), so the scenarios cannot record it: `foo (stmt ...)` with a second tNL or a
/// `;` after a newline before its `)` (#503), a leading `&.` on the line after `def recv` (#504).
/// Each row checked against RuboCop 1.91.0.
#[test]
fn legacy_lexer_newlines_reject_as_the_parser_gem() {
    let rows: &[(&str, &str, bool)] = &[
        ("3.2", "foo (a #c\n\n)\n", true),
        ("3.2", "foo (a\n#c\n\n)\n", true),
        ("2.6", "foo (a\n#c\n)\n", true),
        ("3.2", "foo (a\n#c\n)\n", false),
        ("3.2", "foo (a\n;)\n", true),
        ("3.2", "foo (a\n \n)\n", false),
        ("2.6", "foo (a\n \n)\n", true),
        ("3.2", "foo (a\n\\\n\n)\n", false),
        ("3.2", "foo (<<X\nb\n\nX\n)\n", false),
        ("3.2", "foo (<<X\nb\nX\n\n)\n", true),
        ("3.2", "def self\n&.foo; end\n", false),
        ("2.6", "def (x)\n&.foo; end\n", false),
        ("3.2", "def self # c\n&.foo; end\n", false),
        ("3.2", "def self\n# c\n&.foo; end\n", false),
        ("2.6", "def self\n# c\n&.foo; end\n", true),
        ("3.2", "def self\n\n&.foo; end\n", true),
    ];
    let dir = scratch("legacylexer");
    for &(target, src, rejects) in rows {
        let cfg = format!("AllCops:\n  NewCops: disable\n  SuggestExtensions: false\n  TargetRubyVersion: {target}\n");
        fs::write(dir.join(".rubocop.yml"), cfg).unwrap();
        fs::write(dir.join("t.rb"), src).unwrap();
        let out = text(&rbcop(&dir, &["--only", "Lint/Syntax", "-f", "e", "t.rb"]).stdout);
        assert_eq!(out.contains("Lint/Syntax"), rejects, "{target} {src:?}: {out}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// rubocop-rails' MigrationFileSkippable skips add_offense only (#535): an already-migrated file
/// still runs every cop, so its cop errors and directive warnings come out, but it gets no
/// offense and no correction.
#[test]
fn migrated_file_still_runs_cops() {
    let dir = scratch("migrated");
    let config = "plugins: [rubocop-rails]\nAllCops:\n  NewCops: disable\n  SuggestExtensions: false\n  \
                  MigratedSchemaVersion: \"20240101000000\"\nStyle/Documentation:\n  AllowedConstants: Foo, Bar\n";
    fs::write(dir.join(".rubocop.yml"), config).unwrap();
    fs::create_dir_all(dir.join("db/migrate")).unwrap();
    let source = "# frozen_string_literal: true\n\nclass CreateX < ActiveRecord::Migration[7.0]\n  def change; end\nend\n\
                  puts 1 # rubocop:disable Debugger\nfoo \"a\"\n";
    let (old, new) = ("db/migrate/20200101000000_create_x.rb", "db/migrate/20250101000000_create_y.rb");
    for args in [&["-f", "e"][..], &["-f", "e", "-a"]] {
        fs::write(dir.join(old), source).unwrap();
        fs::write(dir.join(new), source).unwrap();
        let out = rbcop(&dir, args);
        let stderr = text(&out.stderr).replace(&format!("{}/", dir.display()), "");
        let printed: Vec<&str> = stderr.lines().take_while(|l| !l.is_empty()).collect();
        let warning = |f: &str| {
            format!(
                "{f}: Warning: no department given for Debugger. Run `rubocop -a --only Migration/DepartmentName` to fix."
            )
        };
        let error = |f: &str| format!("An error occurred while Style/Documentation cop was inspecting {f}:3:0.");
        for want in [warning(old), error(old), warning(new), error(new)] {
            assert!(printed.contains(&want.as_str()), "{args:?}: {stderr}");
        }
        let stdout = text(&out.stdout);
        assert!(!stdout.contains("20200101000000"), "{args:?}: {stdout}");
        assert!(stdout.contains("20250101000000"), "{args:?}: {stdout}");
        assert_eq!(fs::read_to_string(dir.join(old)).unwrap(), source);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A file's `no department given` warnings print when its CommentConfig is first analyzed (#545):
/// during the investigation when a cop reads it (any offense, Lint/MissingCopEnableDirective),
/// so before the cop errors; else after them, in Runner#check_for_redundant_disables?, which
/// --except Lint/RedundantCopDisableDirective skips. W = the warning, E = the cop error (RuboCop
/// 1.91.0).
#[test]
fn directive_warnings_print_when_comment_config_is_read() {
    let dir = scratch("warnorder");
    let config = "AllCops:\n  NewCops: disable\n  SuggestExtensions: false\nStyle/Documentation:\n  AllowedConstants: Foo, Bar\n";
    fs::write(dir.join(".rubocop.yml"), config).unwrap();
    let source = "# frozen_string_literal: true\n\nclass X\n  def change; end\nend\nputs \"x\" # rubocop:disable StringLiterals\n";
    let rows: &[(&[&str], &str)] = &[
        (&[], "WE"),
        (&["--only", "Style/Documentation"], "EW"),
        (&["--only", "Style/Documentation", "--except", "Lint/RedundantCopDisableDirective"], "E"),
        (&["--only", "Style/Documentation,Style/StringLiterals"], "WE"),
        (&["--ignore-disable-comments", "--only", "Style/Documentation,Style/StringLiterals"], "EW"),
        (&["-a", "--only", "Style/Documentation,Migration/DepartmentName"], "WE"),
        (&["-a", "--ignore-disable-comments", "--only", "Style/Documentation,Style/StringLiterals"], "EW"),
    ];
    for &(args, want) in rows {
        fs::write(dir.join("t.rb"), source).unwrap();
        let args: Vec<&str> = ["-f", "e"].iter().chain(args).chain(&["t.rb"]).copied().collect();
        let stderr = text(&rbcop(&dir, &args).stderr);
        let got: String = stderr
            .lines()
            .take_while(|l| !l.is_empty())
            .filter_map(|l| {
                if l.starts_with("t.rb: Warning: no department given") {
                    Some('W')
                } else if l.starts_with("An error occurred while Style/Documentation") {
                    Some('E')
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(got, want, "{args:?}: {stderr}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// AmbiguousCopName (#633): Metrics/ParameterLists raises in on_def and again in on_args, so a
/// def over both limits has two cop errors (the def, the args).
#[test]
fn parameter_lists_raises_per_callback_under_ambiguous_cop_name() {
    let dir = scratch("ambiguous-parameter-lists");
    let config = "plugins: [rubocop-rails, rubocop-rspec]\nAllCops:\n  NewCops: enable\n  SuggestExtensions: false\n";
    fs::write(dir.join(".rubocop.yml"), config).unwrap();
    let src = "# frozen_string_literal: true\n\n# rubocop:disable Output\ndef m(a, b, c, d, e, f, g = 1, h = 2, i = 3, j = 4); end\n";
    fs::write(dir.join("a.rb"), src).unwrap();
    let stderr = text(&rbcop(&dir, &["-f", "e", "a.rb"]).stderr);
    let mut errors: Vec<&str> =
        stderr.lines().filter(|l| l.starts_with("An error occurred while Metrics/ParameterLists")).collect();
    errors.sort_unstable();
    errors.dedup();
    assert!(errors.len() == 2 && errors[0].ends_with("a.rb:4:0.") && errors[1].ends_with("a.rb:4:5."), "{stderr}");
    let _ = fs::remove_dir_all(&dir);
}
