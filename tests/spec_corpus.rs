//! RuboCop's own spec examples for every cop (tests/spec_corpus, recorded by
//! scripts/record-spec-corpus.sh; format in scripts/record_spec_corpus.rb): for each cop and config
//! variant, `rbcop --only <cop>` must report what RuboCop did, and `rbcop -A --only <cop>` must report
//! and rewrite the files the way RuboCop did.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

type Sections = Vec<(String, Vec<u8>)>;

/// `==> <name> <bytes> <==` header, the content, a newline; repeated.
fn sections(path: &Path) -> Sections {
    let data = fs::read(path).unwrap();
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let eol = i + data[i..].iter().position(|&b| b == b'\n').unwrap();
        let header = std::str::from_utf8(&data[i..eol]).unwrap();
        let inner = header.strip_prefix("==> ").and_then(|h| h.strip_suffix(" <=="));
        let (name, len) = inner
            .and_then(|h| h.rsplit_once(' '))
            .unwrap_or_else(|| panic!("{}: bad header {header:?}", path.display()));
        let len: usize = len.parse().unwrap();
        out.push((name.to_string(), data[eol + 1..eol + 1 + len].to_vec()));
        i = eol + 1 + len + 1;
    }
    out
}

/// Line diff (`-` expected, `+` actual) by longest common subsequence; the outputs are small.
fn diff(expected: &str, actual: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, String::new());
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            (i, j) = (i + 1, j + 1);
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out += &format!("  + {}\n", b[j]);
            j += 1;
        } else {
            out += &format!("  - {}\n", a[i]);
            i += 1;
        }
    }
    if out.is_empty() {
        out = "  (differs only in trailing newline)\n".into();
    }
    out
}

fn run(dir: &Path, cop: &str, autocorrect: bool) -> (String, String) {
    let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
    if autocorrect {
        cmd.arg("-A");
    }
    // `--only` refuses this cop; its .rubocop.yml enables only it (scripts/extract_cold_corpus.rb).
    if cop != "Lint/RedundantCopDisableDirective" {
        cmd.args(["--only", cop]);
    }
    let out = cmd.current_dir(dir).args(["--config", ".rubocop.yml", "--format", "emacs", "."]).output().unwrap();
    let prefix = format!("{}/", dir.canonicalize().unwrap().display());
    let stdout = String::from_utf8_lossy(&out.stdout).replace(&prefix, "");
    let status = out.status.code().map_or("signal".into(), |c| c.to_string());
    (stdout, status)
}

/// Every mismatch in one variant (`Dept_Cop/variant`), empty if it matches.
fn check(root: &Path, variant: &str, work: &Path) -> Vec<String> {
    let cop = variant.split('/').next().unwrap().replacen('_', "/", 1);
    let input = sections(&root.join(format!("{variant}.input")));
    let expected = sections(&root.join(format!("{variant}.expected")));
    let _ = fs::remove_dir_all(work);
    for (name, body) in &input {
        let path = work.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    let mut fails = Vec::new();
    let mut compare = |what: &str, want: &[u8], got: &[u8]| {
        if want != got {
            let (want, got) = (String::from_utf8_lossy(want), String::from_utf8_lossy(got));
            fails.push(format!("{variant} {what} (- rubocop, + rbcop):\n{}", diff(&want, &got)));
        }
    };
    for (i, (label, autocorrect)) in [("offenses", false), ("autocorrect", true)].into_iter().enumerate() {
        let (name, want) = &expected[i];
        let (got, status) = run(work, &cop, autocorrect);
        compare(label, want, got.as_bytes());
        compare(&format!("{label} exit status"), name.as_bytes(), format!("{label} exit={status}").as_bytes());
    }
    for (name, original) in &input[1..] {
        let want =
            expected.iter().find(|(n, _)| n.strip_prefix("corrected/") == Some(name)).map_or(original, |(_, b)| b);
        compare(&format!("-A rewrite of {name}"), want, &fs::read(work.join(name)).unwrap());
    }
    fails
}

#[test]
fn spec_corpus_matches_rubocop() {
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/spec_corpus"));
    let mut variants: Vec<String> = Vec::new();
    for cop in fs::read_dir(&root).unwrap().flatten().filter(|e| e.path().is_dir()) {
        for f in fs::read_dir(cop.path()).unwrap().flatten() {
            if let Some(v) = f.file_name().to_str().unwrap().strip_suffix(".input") {
                variants.push(format!("{}/{v}", cop.file_name().to_str().unwrap()));
            }
        }
    }
    variants.sort();
    assert!(variants.len() > 1000, "spec corpus missing: {} variants", variants.len());

    let tmp = std::env::temp_dir().join(format!("rbcop-spec-corpus-{}", std::process::id()));
    let next = AtomicUsize::new(0);
    let fails = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for t in 0..threads {
            let (root, tmp, variants, next, fails) = (&root, &tmp, &variants, &next, &fails);
            s.spawn(move || {
                let work = tmp.join(t.to_string());
                while let Some(v) = variants.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let f = check(root, v, &work);
                    fails.lock().unwrap().extend(f);
                }
            });
        }
    });
    let _ = fs::remove_dir_all(&tmp);
    let mut fails = fails.into_inner().unwrap();
    fails.sort();
    assert!(fails.is_empty(), "{} mismatches in {} variants:\n{}", fails.len(), variants.len(), fails.join("\n"));
}
