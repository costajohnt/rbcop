//! Which files to inspect (RuboCop's TargetFinder, §6 of docs/rubocop-config-semantics.md).

use crate::config::{Resolved, Store, expand_path, glob};
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// `only_recognized`: `--only-recognized-file-types` (TargetFinder mode :only_recognized_file_types).
pub fn find(
    store: &mut Store,
    args: &[PathBuf],
    force_exclusion: bool,
    only_recognized: bool,
) -> Result<Vec<PathBuf>, String> {
    let cwd = store.cwd().to_path_buf();
    let pwd_cfg = store.for_dir(&cwd)?;
    let (exts, names) = pwd_cfg.ruby_extensions_and_names();
    let ruby = RubyFile { exts, names };
    let mut files = Vec::new();
    let args: Vec<PathBuf> = if args.is_empty() { vec![cwd.clone()] } else { args.to_vec() };
    for arg in args {
        let abs = expand_path(&arg, &cwd);
        if abs.is_dir() {
            let cfg = store.for_dir(&abs)?;
            let mut all = Vec::new();
            walk(&abs, &cfg, &mut all);
            // Ruby String#sort: byte order of the whole path (`foo-baz.rb` < `foo.rb` < `foo/bar.rb`),
            // not PathBuf's per-component order.
            all.sort_by(|a, b| a.as_os_str().as_bytes().cmp(b.as_os_str().as_bytes()));
            let hidden_base = is_hidden_dir(&abs);
            for f in all {
                let keep = if hidden_base {
                    ruby.is(&f, store)?
                } else {
                    !cfg.file_to_exclude(&f)
                        && ((!hidden_in(&f, &abs) && ruby.is(&f, store)?) || cfg.file_to_include(&f))
                };
                if keep {
                    files.push(f);
                }
            }
        } else {
            // TargetFinder#process_explicit_path (a missing file too: the caller fails reading it):
            // included_file? under either option, then
            // without_excluded under --force-exclusion. included_file? comes first: for an
            // extensionless file ruby_executable? loads the file's directory config, excluded or not.
            if force_exclusion || only_recognized {
                let cfg = store.for_dir(&cwd)?;
                if !(ruby.is(&abs, store)? || cfg.file_to_include(&abs))
                    || (force_exclusion && cfg.file_to_exclude(&abs))
                {
                    continue;
                }
            }
            files.push(abs);
        }
    }
    let mut seen = std::collections::HashSet::new();
    files.retain(|f| seen.insert(f.clone()));
    Ok(files)
}

struct RubyFile {
    exts: Vec<String>,
    names: Vec<String>,
}

impl RubyFile {
    /// TargetFinder#ruby_file?
    fn is(&self, f: &Path, store: &mut Store) -> Result<bool, String> {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = f.extension().map(|e| format!(".{}", e.to_string_lossy()));
        Ok(ext.as_ref().is_some_and(|e| self.exts.contains(e))
            // ruby_filename?: `ruby_filenames.include?(File.basename(file))`, a literal compare.
            || self.names.contains(&name)
            || (ext.is_none() && executable(f, store)?))
    }
}

/// TargetFinder#ruby_executable?: an extensionless, non-empty file whose first line matches
/// `#!.*(<RubyInterpreters>)`, the interpreters coming from the file's own directory config
/// (ConfigStore#for(file)), which is loaded (and validated, so an invalid one ends the run before
/// anything is reported) for every such file.
fn executable(f: &Path, store: &mut Store) -> Result<bool, String> {
    let Ok(file) = fs::File::open(f) else { return Ok(false) };
    let mut first = Vec::new();
    if BufReader::new(file).read_until(b'\n', &mut first).is_err() || first.is_empty() {
        return Ok(false);
    }
    // readline raises ArgumentError on an invalid byte sequence (rescued: not Ruby).
    let Ok(first) = String::from_utf8(first) else { return Ok(false) };
    let interpreters = store.for_dir(f.parent().unwrap_or(Path::new("/")))?.ruby_interpreters();
    let line = first.trim_end_matches('\n');
    Ok(line.find("#!").is_some_and(|i| {
        let rest = &line[i + 2..];
        interpreters.is_empty() || interpreters.iter().any(|x| rest.contains(x.as_str()))
    }))
}

/// Every file under `dir` (dotfiles included), pruning directories excluded by `.../**/*`
/// patterns. Like TargetFinder#find_files, symlinked files are kept and symlinked directories are
/// followed, unless the link's target is excluded or contains the directory being walked (a loop).
///
/// Deliberate difference: iterative, and a symlinked directory that is already one of the walked
/// directory's ancestors (by device and inode) is not entered. RuboCop's prefix test misses cycles
/// through two links (`a/l -> ../b`, `b/l -> ../a`) and expands them until the OS refuses (ELOOP),
/// listing the same files under ever longer paths.
fn walk(root: &Path, cfg: &Resolved, out: &mut Vec<PathBuf>) {
    use std::os::unix::fs::MetadataExt;
    let prune: Vec<String> = cfg
        .all_cops()
        .strings("Exclude")
        .into_iter()
        .filter_map(|p| p.strip_suffix("/**/*").map(String::from))
        .collect();
    let pruned = |p: &Path| prune.iter().any(|x| glob::fnmatch(x, &p.display().to_string(), true));
    let id = |m: &fs::Metadata| (m.dev(), m.ino());
    let Ok(root_meta) = fs::metadata(root) else { return };
    // (directory, (dev, ino) of it and every directory above it in this walk)
    let mut stack = vec![(root.to_path_buf(), vec![id(&root_meta)])];
    while let Some((dir, chain)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            let link = kind.is_symlink();
            let Ok(meta) = (if link { fs::metadata(&path) } else { entry.metadata() }) else { continue };
            if meta.is_dir() {
                if pruned(&path) || chain.contains(&id(&meta)) {
                    continue;
                }
                // symlink_excluded_or_infinite_loop? (a string prefix test, as RuboCop's start_with?).
                if link {
                    let (Ok(real), Ok(here)) = (fs::canonicalize(&path), fs::canonicalize(&dir)) else { continue };
                    if pruned(&real) || here.display().to_string().starts_with(&real.display().to_string()) {
                        continue;
                    }
                }
                let mut below = chain.clone();
                below.push(id(&meta));
                stack.push((path, below));
            } else if meta.is_file() {
                out.push(path);
            }
        }
    }
}

fn is_hidden_dir(dir: &Path) -> bool {
    dir.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// TargetFinder#hidden_file_in_dir?: any path component below `base` starts with a dot.
fn hidden_in(file: &Path, base: &Path) -> bool {
    let rel = file.strip_prefix(base).unwrap_or(file).display().to_string();
    rel.starts_with('.') || rel.contains("/.")
}

/// ChangedFiles#paths (`--changed REVISION`): the files added or modified since `revision` and the
/// untracked ones, as absolute paths. Deleted files are left out. Errors are RuboCop::Error messages.
/// With a repository pinned (`pin_repository`), only that one is asked, and none if there was none.
pub fn changed(revision: &str) -> Result<std::collections::HashSet<PathBuf>, String> {
    if let Some(Err(e)) = PINNED.get() {
        return Err(e.clone());
    }
    let root = git(&["rev-parse", "--show-toplevel"])?;
    // String#chomp
    let root = root.strip_suffix(b"\n").map(|r| r.strip_suffix(b"\r").unwrap_or(r)).unwrap_or(&root);
    let root = Path::new(std::ffi::OsStr::from_bytes(root));
    // `diff.relative` would make git report paths relative to the current directory.
    // Deliberate difference: `--end-of-options` keeps a revision such as `--output=FILE` from
    // being read as a git option (RuboCop 1.92.2 passes it through, and git writes FILE). Git's
    // messages for every other revision are unchanged; a trailing `--` would reword them.
    // `--no-ext-diff --no-textconv --no-renames`: see git_command (a rename's new name is listed
    // either way, and its old one never is).
    let diff = |revision: &str| {
        git(&[
            "-c",
            "diff.relative=false",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--name-only",
            "--diff-filter=d",
            "-z",
            "--end-of-options",
            revision,
        ])
    };
    let modified = match diff(revision) {
        Ok(out) => out,
        // A repository without commits: against the empty tree every staged file counts as changed.
        Err(_) if revision == "HEAD" && !git_ok(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]) => {
            diff(&empty_tree()?)?
        }
        Err(e) => return Err(e),
    };
    // `:/` widens ls-files (which only looks under the current directory) to the whole repository.
    let untracked = git(&["ls-files", "--others", "--exclude-standard", "--full-name", "-z", "--", ":/"])?;
    Ok(modified
        .split(|&b| b == 0)
        .chain(untracked.split(|&b| b == 0))
        .filter(|p| !p.is_empty())
        .map(|p| expand_path(Path::new(std::ffi::OsStr::from_bytes(p)), root))
        .collect())
}

/// ChangedFiles#empty_tree: hashed, since its id depends on the object format (SHA-1 or SHA-256).
fn empty_tree() -> Result<String, String> {
    if let Some(Ok(p)) = PINNED.get() {
        return Ok(p.empty_tree.clone());
    }
    let out = git(&["hash-object", "-t", "tree", "--stdin"])?;
    // String#chomp
    let out = String::from_utf8_lossy(&out);
    Ok(out.strip_suffix('\n').map_or(&*out, |o| o.strip_suffix('\r').unwrap_or(o)).to_string())
}

/// The repository `--mcp` resolved at startup, as canonical paths: its git dir, its common dir (a
/// linked worktree's main repository) and its work tree; and the empty tree's id in its object
/// format.
pub struct Pinned {
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
    pub toplevel: PathBuf,
    empty_tree: String,
}

/// Set once by `pin_repository`: the repository, or the error `--changed` gives without one.
static PINNED: std::sync::OnceLock<Result<Pinned, String>> = std::sync::OnceLock::new();

/// Deliberate difference (`--mcp`): the working directory's repository is resolved once, before
/// any request, and from then on git is pointed at it (GIT_DIR, GIT_WORK_TREE,
/// GIT_CEILING_DIRECTORIES), so a repository a client plants later, or below the root, is never
/// discovered. With none at startup, `changed` keeps failing as it does outside a repository.
pub fn pin_repository() -> Option<&'static Pinned> {
    PINNED.get_or_init(discover).as_ref().ok()
}

fn discover() -> Result<Pinned, String> {
    let out = git(&["rev-parse", "--absolute-git-dir", "--git-common-dir", "--show-toplevel"])?;
    let out = String::from_utf8_lossy(&out);
    let cwd = crate::config::cwd();
    let canonical = |p: &str| {
        let p = expand_path(Path::new(p), &cwd);
        fs::canonicalize(&p).unwrap_or(p)
    };
    let mut lines = out.lines().map(canonical);
    let (Some(git_dir), Some(common_dir), Some(toplevel)) = (lines.next(), lines.next(), lines.next()) else {
        return Err(
            "--changed could not ask git which files changed: git failed with no output on standard error.".into()
        );
    };
    // Fail closed: without GIT_ATTR_SOURCE (git 2.40) a `.gitattributes` could name a filter.
    let version = git(&["version"])?;
    let version = String::from_utf8_lossy(&version);
    let numbers: Vec<u32> =
        version.split_whitespace().nth(2).unwrap_or("").split('.').take(2).map(|n| n.parse().unwrap_or(0)).collect();
    if numbers.as_slice() < [2, 40].as_slice() {
        return Err(format!("`changed` needs git 2.40 or newer under rbcop --mcp ({}).", version.trim()));
    }
    let mut pinned = Pinned { git_dir, common_dir, toplevel, empty_tree: String::new() };
    let mut cmd = git_command(&["hash-object", "-t", "tree", "--stdin"]);
    pin(&mut cmd, &pinned);
    let out = cmd.output().ok().filter(|o| o.status.success());
    let tree = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    if !matches!(tree.len(), 40 | 64) || !tree.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("`changed` could not pin the repository: git hash-object gave no empty tree id.".into());
    }
    pinned.empty_tree = tree;
    Ok(pinned)
}

/// The pinned repository's environment. Its attributes are read from the empty tree, so a
/// `.gitattributes` a client wrote cannot name a filter driver for git to run.
fn pin(cmd: &mut std::process::Command, p: &Pinned) {
    cmd.env("GIT_DIR", &p.git_dir).env("GIT_WORK_TREE", &p.toplevel);
    if let Some(parent) = p.toplevel.parent() {
        cmd.env("GIT_CEILING_DIRECTORIES", parent);
    }
    // While pinning, the empty tree's id is not known yet (only hash-object runs then).
    if !p.empty_tree.is_empty() {
        cmd.env("GIT_ATTR_SOURCE", &p.empty_tree);
    }
}

/// ChangedFiles#git: git's stdout, or why it failed.
fn git(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = git_command(args).output().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "--changed requires git to be installed and on your PATH.".to_string(),
        _ => e.to_string(),
    })?;
    if out.status.success() {
        return Ok(out.stdout);
    }
    // String#strip
    let stderr = String::from_utf8_lossy(&out.stderr);
    let message = stderr.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0');
    let message = if message.is_empty() { "git failed with no output on standard error." } else { message };
    Err(format!("--changed could not ask git which files changed: {message}"))
}

fn git_ok(args: &[&str]) -> bool {
    git_command(args).output().is_ok_and(|o| o.status.success())
}

/// Open3.capture3: stdout and stderr captured, stdin an empty pipe.
///
/// Deliberate difference: git runs with `core.fsmonitor` and `core.hooksPath` off, implicit bare
/// repositories refused (`safe.bareRepository=explicit`), no transport (`protocol.allow=never`,
/// GIT_NO_LAZY_FETCH), no global or system attributes file, no system config, no external diff,
/// textconv or pager, and the pinned repository when there is one. The file lists git prints are
/// the same.
pub fn git_command(args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.args([
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "safe.bareRepository=explicit",
        "-c",
        "protocol.allow=never",
        "-c",
        "core.attributesFile=/dev/null",
    ])
    .args(args)
    .env("GIT_CONFIG_NOSYSTEM", "1")
    .env("GIT_ATTR_NOSYSTEM", "1")
    .env("GIT_NO_LAZY_FETCH", "1")
    .env("GIT_PAGER", "cat")
    .env_remove("GIT_EXTERNAL_DIFF")
    .stdin(std::process::Stdio::null());
    if let Some(Ok(p)) = PINNED.get() {
        pin(&mut cmd, p);
    }
    cmd
}
