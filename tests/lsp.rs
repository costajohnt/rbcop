//! `--lsp` transcripts: rbcop must answer tests/lsp/*/input.jsonl with exactly the messages
//! `rubocop --lsp` sent (expected.jsonl and status, recorded by scripts/lsp.sh).

mod common;

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

/// The bodies of the Content-Length framed messages in `out`, each checked against its header.
fn bodies(mut out: &[u8]) -> Vec<String> {
    let mut list = Vec::new();
    while !out.is_empty() {
        let sep = out.windows(4).position(|w| w == b"\r\n\r\n").expect("a header");
        let header = std::str::from_utf8(&out[..sep]).unwrap();
        let n: usize = header.strip_prefix("Content-Length: ").expect("Content-Length").parse().unwrap();
        list.push(String::from_utf8(out[sep + 4..sep + 4 + n].to_vec()).unwrap());
        out = &out[sep + 4 + n..];
    }
    list
}

#[test]
fn lsp_transcripts_match_rubocop() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lsp");
    let mut names: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    assert!(!names.is_empty());
    for dir in names {
        let project = dir.join("project").canonicalize().unwrap();
        let ids = project.display().to_string();
        let mut input = Vec::new();
        for line in std::fs::read_to_string(dir.join("input.jsonl")).unwrap().lines() {
            let m = line.replace("$ROOT", &ids);
            write!(input, "Content-Length: {}\r\n\r\n{m}", m.len()).unwrap();
        }
        let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .arg("--lsp")
            .current_dir(&project)
            .env("PWD", &project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // The server may exit before reading everything (shutdown): ignore a broken pipe.
        let _ = child.stdin.take().unwrap().write_all(&input);
        let out = child.wait_with_output().unwrap();
        let actual: Vec<String> = bodies(&out.stdout).iter().map(|b| b.replace(&ids, "$ROOT")).collect();
        let expected: Vec<String> =
            std::fs::read_to_string(dir.join("expected.jsonl")).unwrap().lines().map(String::from).collect();
        let name = dir.file_name().unwrap().to_string_lossy();
        for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(a, e, "{name}: message {}", i + 1);
        }
        assert_eq!(actual.len(), expected.len(), "{name}: message count");
        let status = std::fs::read_to_string(dir.join("status")).unwrap();
        assert_eq!(out.status.code(), status.trim().parse().ok(), "{name}: exit status");
    }
}
