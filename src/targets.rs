//! Which files to inspect (RuboCop's TargetFinder, §6 of docs/rubocop-config-semantics.md).

use crate::config::{Resolved, Store, expand_path, glob};
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub fn find(store: &mut Store, args: &[PathBuf], force_exclusion: bool) -> Result<Vec<PathBuf>, String> {
    let cwd = store.cwd().to_path_buf();
    let pwd_cfg = store.for_dir(&cwd)?;
    let (exts, names) = pwd_cfg.ruby_extensions_and_names();
    let ruby = RubyFile { exts, names, interpreters: pwd_cfg.ruby_interpreters() };
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
                    ruby.is(&f)
                } else {
                    !cfg.file_to_exclude(&f) && ((!hidden_in(&f, &abs) && ruby.is(&f)) || cfg.file_to_include(&f))
                };
                if keep {
                    files.push(f);
                }
            }
        } else if abs.exists() {
            if force_exclusion {
                let cfg = store.for_dir(&cwd)?;
                if cfg.file_to_exclude(&abs) || !(ruby.is(&abs) || cfg.file_to_include(&abs)) {
                    continue;
                }
            }
            files.push(abs);
        } else {
            return Err(format!("{}: No such file or directory", arg.display()));
        }
    }
    let mut seen = std::collections::HashSet::new();
    files.retain(|f| seen.insert(f.clone()));
    Ok(files)
}

struct RubyFile {
    exts: Vec<String>,
    names: Vec<String>,
    interpreters: Vec<String>,
}

impl RubyFile {
    /// TargetFinder#ruby_file?
    fn is(&self, f: &Path) -> bool {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = f.extension().map(|e| format!(".{}", e.to_string_lossy()));
        ext.as_ref().is_some_and(|e| self.exts.contains(e))
            // ruby_filename?: `ruby_filenames.include?(File.basename(file))`, a literal compare.
            || self.names.contains(&name)
            || (ext.is_none() && self.executable(f))
    }

    /// Extensionless, non-empty, first line `#!.*(ruby|rake|jruby)`.
    fn executable(&self, f: &Path) -> bool {
        let Ok(file) = fs::File::open(f) else { return false };
        let mut first = String::new();
        if BufReader::new(file).read_line(&mut first).is_err() || !first.starts_with("#!") {
            return false;
        }
        self.interpreters.iter().any(|i| first.contains(i.as_str()))
    }
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
