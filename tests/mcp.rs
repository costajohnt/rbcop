//! `--mcp` transcripts: rbcop, run in a copy of tests/mcp/*/project, must answer input.jsonl with
//! exactly the lines `rubocop --mcp` wrote (expected.jsonl and status, recorded by scripts/mcp.sh).

mod common;

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn mcp_transcripts_match_rubocop() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mcp");
    let mut names: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    assert!(!names.is_empty());
    for dir in names {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        // The autocorrection tool writes files: run in a scratch copy of the project.
        let scratch = std::env::temp_dir().join(format!("rbcop-mcp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        copy_dir(&dir.join("project"), &scratch);
        let project = scratch.canonicalize().unwrap();
        let ids = project.display().to_string();
        let input = std::fs::read_to_string(dir.join("input.jsonl")).unwrap().replace("$ROOT", &ids);
        let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .arg("--mcp")
            .current_dir(&project)
            .env("PWD", &project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        let _ = std::fs::remove_dir_all(&scratch);
        let actual: Vec<String> =
            String::from_utf8(out.stdout).unwrap().lines().map(|l| l.replace(&ids, "$ROOT")).collect();
        let expected: Vec<String> =
            std::fs::read_to_string(dir.join("expected.jsonl")).unwrap().lines().map(String::from).collect();
        for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(a, e, "{name}: line {}", i + 1);
        }
        assert_eq!(actual.len(), expected.len(), "{name}: line count");
        let status = std::fs::read_to_string(dir.join("status")).unwrap();
        assert_eq!(out.status.code(), status.trim().parse().ok(), "{name}: exit status");
    }
}
