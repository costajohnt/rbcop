//! `--changed [REVISION]` (Runner#only_changed, ChangedFiles): output recorded with RuboCop 1.91.0
//! on the same throwaway git repositories.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

/// Git with only the configuration the test writes (`gitconfig` under the test's directory).
fn git(dir: &Path, args: &[&str]) {
    let config = dir.ancestors().find(|d| d.join("gitconfig").is_file()).unwrap().join("gitconfig");
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

const CONFIG: &str = "AllCops:\n  NewCops: enable\n  SuggestExtensions: false\n  Exclude:\n    - \"skip.rb\"\n";
const NO_REVISION: &str = "Error: --changed could not ask git which files changed: fatal: ambiguous argument 'nope': \
                           unknown revision or path not in the working tree.\nUse '--' to separate paths from \
                           revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'\n";

/// (directory, arguments, stdout, stderr, exit status); `{dir}/` is taken out of stdout.
fn check(top: &Path, cases: &[(&str, &[&str], &str, &str, i32)]) {
    let mut failures = Vec::new();
    for &(sub, args, stdout, stderr, status) in cases {
        let dir = top.join(sub);
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(["--cache", "false"])
            .args(args)
            .current_dir(&dir)
            .env("PWD", &dir)
            // The directory without a repository must not find one above it.
            .env("GIT_CEILING_DIRECTORIES", top)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        let got_out = String::from_utf8_lossy(&out.stdout).replace(&format!("{}/", dir.display()), "");
        let got_err = String::from_utf8_lossy(&out.stderr);
        if (got_out.as_str(), got_err.as_ref(), out.status.code()) != (stdout, stderr, Some(status)) {
            failures.push(format!(
                "{sub} {args:?}\n  stdout: {got_out:?}\n  want:   {stdout:?}\n  stderr: {got_err:?}\n  want:   \
                 {stderr:?}\n  exit {:?} want {status}",
                out.status.code()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn changed_matches_rubocop() {
    let top = std::env::temp_dir().join(format!("rbcop-changed-{}", std::process::id()));
    let _ = fs::remove_dir_all(&top);
    let repo = top.join("repo");
    fs::create_dir_all(&repo).unwrap();
    write(
        &top,
        &[(
            "gitconfig",
            "[user]\n\tname = t\n\temail = t@example.invalid\n[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n",
        )],
    );
    write(
        &repo,
        &[
            (".rubocop.yml", CONFIG),
            ("Gemfile", "\n"),
            ("a.rb", "x = 1\n"),
            ("b.rb", "y = 2\n"),
            ("gone.rb", "z = 3\n"),
            ("sub/f.rb", "w = 4\n"),
            ("skip.rb", "s = 1\n"),
            ("old.rb", "old = 1\n"),
            (".gitignore", "ignored.rb\n"),
        ],
    );
    git(&repo, &["init", "-q", "."]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "one"]);
    write(&repo, &[("old.rb", "old = 2\n")]);
    git(&repo, &["commit", "-qam", "two"]);
    git(&repo, &["tag", "a.rb"]);
    // Modified, deleted, untracked and ignored files.
    write(
        &repo,
        &[
            ("b.rb", "y = 22\n"),
            ("sub/f.rb", "w = 44\n"),
            ("skip.rb", "s = 2\n"),
            ("c.rb", "c = 1\n"),
            ("ignored.rb", "i = 1\n"),
        ],
    );
    fs::remove_file(repo.join("gone.rb")).unwrap();

    let norepo = top.join("norepo");
    let unborn = top.join("unborn");
    for dir in [&norepo, &unborn] {
        write(dir, &[(".rubocop.yml", CONFIG), ("a.rb", "x = 1\n")]);
    }
    git(&unborn, &["init", "-q", "."]);

    let lint = "b.rb:1:1: W: [Correctable] Lint/UselessAssignment: Useless assignment to variable - `y`.\n\
                b.rb:1:1: C: [Correctable] Style/FrozenStringLiteralComment: Missing frozen string literal comment.\n\
                c.rb:1:1: W: [Correctable] Lint/UselessAssignment: Useless assignment to variable - `c`.\n\
                c.rb:1:1: C: [Correctable] Style/FrozenStringLiteralComment: Missing frozen string literal comment.\n\
                sub/f.rb:1:1: W: [Correctable] Lint/UselessAssignment: Useless assignment to variable - `w`.\n\
                sub/f.rb:1:1: C: [Correctable] Style/FrozenStringLiteralComment: Missing frozen string literal comment.\n";
    let empty_revision = NO_REVISION.replace("'nope'", "''");
    let unborn_main = NO_REVISION.replace("'nope'", "'main'");
    check(
        &top,
        &[
            ("repo", &["--changed", "-L"], "b.rb\nc.rb\nsub/f.rb\n", "", 0),
            ("repo", &["-L", "--changed"], "b.rb\nc.rb\nsub/f.rb\n", "", 0),
            ("repo", &["--changed=HEAD~1", "-L"], "b.rb\nc.rb\nold.rb\nsub/f.rb\n", "", 0),
            ("repo", &["--changed=HEAD", "-L", "sub"], "sub/f.rb\n", "", 0),
            ("repo", &["--changed", "-L", "--", "skip.rb", "a.rb", "b.rb", "missing.rb"], "skip.rb\nb.rb\n", "", 0),
            ("repo", &["--changed", "-L", "--force-exclusion", "skip.rb", "b.rb"], "b.rb\n", "", 0),
            ("repo", &["--changed", "--format", "emacs"], lint, "", 1),
            ("repo", &["--changed=nope", "-L"], "", NO_REVISION, 2),
            // Options#return_path_given_to_changed (RuboCop 1.92.1): a path given to --changed is
            // inspected, unless it is also a revision (the tag `a.rb`).
            ("repo", &["--changed", "sub", "-L"], "sub/f.rb\n", "", 0),
            ("repo", &["--changed=sub", "-L"], "sub/f.rb\n", "", 0),
            ("repo", &["--changed", "sub", "b.rb", "-L"], "sub/f.rb\nb.rb\n", "", 0),
            (
                "repo",
                &["--changed", "a.rb", "-L"],
                "",
                "--changed got `a.rb`, which is both a path and a git revision. Write `./a.rb` for the path, or \
                 `--changed=a.rb^0` for the revision.\n",
                2,
            ),
            ("repo", &["--changed=", "-L"], "", &empty_revision, 2),
            ("repo", &["--changed", "-s", "a.rb"], "", "--changed cannot be used with --stdin.\n", 2),
            ("repo/sub", &["--changed", "-L"], "f.rb\n", "", 0),
            ("repo/sub", &["--changed", "-L", ".."], "../b.rb\nf.rb\n", "", 0),
            (
                "norepo",
                &["--changed", "-L"],
                "",
                "Error: --changed could not ask git which files changed: fatal: not a git repository (or any of the \
                 parent directories): .git\n",
                2,
            ),
            ("unborn", &["--changed", "-L"], "a.rb\n", "", 0),
            ("unborn", &["--changed=HEAD", "-L"], "a.rb\n", "", 0),
            ("unborn", &["--changed=main", "-L"], "", &unborn_main, 2),
        ],
    );
    let _ = fs::remove_dir_all(&top);
}

/// Deliberate difference (docs/known-gaps.md): RuboCop hands a revision starting with `-` to
/// `git diff` as an option (`--output=FILE` writes FILE); rbcop makes git read it as a revision,
/// from the command line or the `.rubocop` arguments file alike.
#[test]
fn changed_revision_is_never_a_git_option() {
    let top = std::env::temp_dir().join(format!("rbcop-changed-dash-{}", std::process::id()));
    let _ = fs::remove_dir_all(&top);
    let repo = top.join("repo");
    write(&top, &[("gitconfig", "[user]\n\tname = t\n\temail = t@example.invalid\n[commit]\n\tgpgsign = false\n")]);
    write(&repo, &[(".rubocop.yml", CONFIG), ("a.rb", "x = 1\n")]);
    git(&repo, &["init", "-q", "."]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "one"]);
    write(&repo, &[("a.rb", "x = 2\n")]);
    let written = repo.join("written.txt");
    let absolute = format!("--changed=--output={}", written.display());
    for (args, options_file) in [
        (vec!["--changed=--output=written.txt", "-L"], None),
        (vec![absolute.as_str(), "-L"], None),
        (vec!["--changed=-p", "-L"], None),
        (vec!["-L"], Some("--changed=--output=written.txt\n")),
    ] {
        match options_file {
            Some(text) => fs::write(repo.join(".rubocop"), text).unwrap(),
            None => {
                let _ = fs::remove_file(repo.join(".rubocop"));
            }
        }
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(["--cache", "false"])
            .args(&args)
            .current_dir(&repo)
            .env("PWD", &repo)
            .env("GIT_CEILING_DIRECTORIES", &top)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?} {options_file:?}: {stderr}");
        assert!(out.stdout.is_empty(), "{args:?} {options_file:?}");
        assert!(stderr.starts_with("Error: --changed could not ask git which files changed: "), "{stderr}");
        assert!(!written.exists(), "{args:?} {options_file:?}: git wrote {}", written.display());
    }
    let _ = fs::remove_dir_all(&top);
}
