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
        ("a: *x\n", "An alias referenced an unknown anchor: x"),
        ("b: 2020-01-01\n", "Tried to load unspecified class: Date"),
        ("b: 2020-01-01 10:00:00\n", "Tried to load unspecified class: Time"),
        ("a: !ruby/object:Foo {}\n", "Tried to load unspecified class: Foo"),
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

/// A corrector that clobbers (Style/IfInsideElse on a one-line nested if) is a cop error located
/// at the node the cop was visiting (the outer `if`, 1:0), not at the offense (the inner `if`).
#[test]
fn cop_error_is_located_at_the_visited_node() {
    let dir = scratch("coperror");
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: disable\n  TargetRubyVersion: 2.7\n").unwrap();
    fs::write(dir.join("t.rb"), "if cond then foo else if cond2; bar else baz end; end\n").unwrap();
    let out = rbcop(&dir, &["--only", "Style/IfInsideElse", "-f", "e", "t.rb"]);
    let err = text(&out.stderr);
    assert!(err.starts_with("An error occurred while Style/IfInsideElse cop was inspecting t.rb:1:0.\n"), "{err}");
    assert_eq!(out.status.code(), Some(1));
    let _ = fs::remove_dir_all(&dir);
}
