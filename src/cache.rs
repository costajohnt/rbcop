//! The result cache (ResultCache, CachedData, Runner#file_offense_cache): each inspected file's
//! offenses under `<root>/rbcop_cache/<build>/<context>/<file>`, so an unchanged file is not
//! inspected again. rbcop keeps its own `rbcop_cache` next to RuboCop's `rubocop_cache`: each
//! tool's MaxFilesInCache pruning would otherwise delete the other's entries.

use rbcop::cop::{Offense, Severity};
use rbcop::runner::FileReport;
use std::collections::HashSet;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// CacheConfig.root_dir, without its `rubocop_cache` part: `RUBOCOP_CACHE_ROOT`, then `configured`
/// (`--cache-root`, else `AllCops: CacheRootDirectory`), then `$XDG_CACHE_HOME/<uid>`, then
/// `~/.cache`. None when the home directory does not exist, where RuboCop raises
/// (`File.realpath(Dir.home)`).
pub fn root_dir(configured: Option<String>) -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("RUBOCOP_CACHE_ROOT") {
        return Some(root.into());
    }
    if let Some(root) = configured {
        return Some(root.into());
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        // SAFETY: getuid cannot fail.
        return Some(PathBuf::from(xdg).join(unsafe { libc::getuid() }.to_string()));
    }
    // ponytail: HOME unset (Dir.home's getpwuid fallback) runs without a cache.
    let home = std::env::var_os("HOME")?;
    fs::canonicalize(home).ok().map(|h| h.join(".cache"))
}

/// 128 bits of std's SipHash over `parts`. The hasher may change between Rust releases, which is
/// fine: the build digest (the binary's own bytes) is part of every path.
pub fn digest(parts: &[&[u8]]) -> String {
    let one = |seed: u8| {
        let mut h = std::hash::DefaultHasher::new();
        h.write_u8(seed);
        for p in parts {
            h.write_usize(p.len());
            h.write(p);
        }
        h.finish()
    };
    format!("{:016x}{:016x}", one(0), one(1))
}

pub struct Cache {
    /// `<root>/rbcop_cache`, what MaxFilesInCache counts.
    top: PathBuf,
    /// `top/<build>/<context>`.
    dir: PathBuf,
    allow_symlinks: bool,
    max_files: Option<i64>,
    /// Cop names, to give loaded offenses their `&'static str`.
    names: HashSet<&'static str>,
    saved: std::sync::atomic::AtomicUsize,
}

impl Cache {
    /// None when rbcop cannot read its own binary (so cannot tell a rebuilt one from this one).
    pub fn new(
        root: &Path,
        context: &str,
        allow_symlinks: bool,
        max_files: Option<i64>,
        names: HashSet<&'static str>,
    ) -> Option<Cache> {
        // ResultCache.source_checksum: RuboCop hashes its own source; rbcop, its binary.
        // One pass over the binary (tens of MB): 64 bits tell builds apart.
        let mut exe = std::hash::DefaultHasher::new();
        exe.write(&fs::read(std::env::current_exe().ok()?).ok()?);
        let build = digest(&[env!("CARGO_PKG_VERSION").as_bytes(), &exe.finish().to_le_bytes()]);
        let top = root.join("rbcop_cache");
        let dir = top.join(build).join(digest(&[context.as_bytes()]));
        Some(Cache { top, dir, allow_symlinks, max_files, names, saved: Default::default() })
    }

    /// ResultCache#file_checksum: the path, the file mode, everything else that decides the file's
    /// offenses (`signature`) and the contents. None for anything but a regular file.
    pub fn key(&self, path: &Path, signature: &str, bytes: &[u8]) -> Option<String> {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::metadata(path).ok().filter(|m| m.is_file())?;
        Some(digest(&[path.as_os_str().as_encoded_bytes(), &meta.mode().to_le_bytes(), signature.as_bytes(), bytes]))
    }

    /// CachedData#from_json: the report saved for `key`, against `bytes` (the file as read now).
    pub fn load(&self, key: &str, bytes: &[u8]) -> Option<FileReport> {
        // Not through a symlink planted as an entry.
        let mut data = String::new();
        let mut file =
            fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(self.dir.join(key)).ok()?;
        file.read_to_string(&mut data).ok()?;
        let mut lines = data.split('\n');
        let text_len: usize = lines.next()?.parse().ok()?;
        let mut offenses = Vec::new();
        for line in lines.filter(|l| !l.is_empty()) {
            let mut f = line.splitn(6, ' ');
            let (start, end, severity, flags, cop, rest) =
                (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
            // The message, then a tab and the directive's justification when there is one.
            let (message, justification) = match rest.split_once('\t') {
                Some((m, j)) => (m, Some(unescape(j))),
                None => (rest, None),
            };
            let flag = |i: usize| flags.as_bytes().get(i) == Some(&b'1');
            offenses.push(Offense {
                start: start.parse().ok()?,
                end: end.parse().ok()?,
                cop: self.names.get(cop)?,
                message: unescape(message),
                severity: Severity::parse(severity)?,
                correctable: flag(0),
                global: flag(1),
                corrected: false,
                source: None,
                disabled: flag(2),
                justification,
                todo: false,
            });
        }
        // runner::inspect_bytes keeps the source as read for an encoding error, else with CRLF
        // normalised; the saved length tells which.
        let raw = String::from_utf8_lossy(bytes).into_owned();
        let text = if raw.len() == text_len { raw } else { raw.replace("\r\n", "\n") };
        (text.len() == text_len).then_some(FileReport {
            offenses,
            text,
            warnings: Vec::new(),
            abort: None,
            diffs: Vec::new(),
        })
    }

    /// ResultCache#save, reached where Runner#save_in_cache would save. Only writes when `write`
    /// (the caller knows the offenses are those of a plain inspection), but warns as RuboCop does
    /// either way.
    pub fn save(&self, key: &str, report: &FileReport, write: bool) {
        match mkdir_p(&self.dir) {
            Ok(()) => {}
            Err(Some(msg)) => return eprintln!("Couldn't create cache directory. Continuing without cache.\n  {msg}"),
            Err(None) => return,
        }
        // A symbolic link anywhere in the cache directory's path can be a symlink attack.
        if let Some(link) = self.refused_symlink(&self.dir) {
            return eprintln!("Warning: {} is a symlink, which is not allowed.", link.display());
        }
        if !write {
            return;
        }
        let mut out = format!("{}\n", report.text.len());
        for o in &report.offenses {
            let flags = format!("{}{}{}", u8::from(o.correctable), u8::from(o.global), u8::from(o.disabled));
            let mut message = escape(&o.message);
            if let Some(j) = &o.justification {
                message = format!("{message}\t{}", escape(j));
            }
            out.push_str(&format!("{} {} {} {flags} {} {message}\n", o.start, o.end, o.severity.name(), o.cop));
        }
        // Written aside and renamed, so a parallel run never reads half an entry. The temporary
        // name is unpredictable and created fresh (never through an existing file or symlink);
        // any failure only skips saving.
        let n = self.saved.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let random = std::collections::hash_map::RandomState::new().hash_one(n);
        let tmp = self.dir.join(format!("{key}_{}_{random:016x}", std::process::id()));
        let written = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .and_then(|mut f| f.write_all(out.as_bytes()))
            .and_then(|()| fs::rename(&tmp, self.dir.join(key)));
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }

    /// ResultCache#any_symlink?: the deepest symlink in `path` or its ancestors, unless
    /// AllowSymlinksInCacheRootDirectory.
    fn refused_symlink<'p>(&self, path: &'p Path) -> Option<&'p Path> {
        if self.allow_symlinks {
            return None;
        }
        path.ancestors().take_while(|p| p.parent().is_some()).find(|p| p.is_symlink())
    }

    /// ResultCache.cleanup: past MaxFilesInCache entries, remove the oldest half (and one more).
    pub fn cleanup(&self) {
        let Some(max) = self.max_files else { return };
        // Never prune through a symlink: none in the root's path (skipped quietly; RuboCop warns
        // only when saving), and inside the tree only real directories are entered and only
        // regular files removed.
        if self.refused_symlink(&self.top).is_some() {
            return;
        }
        let below = |d: &Path, dirs: bool| -> Vec<PathBuf> {
            fs::read_dir(d)
                .map(|r| {
                    r.flatten()
                        .filter(|e| !e.file_name().as_encoded_bytes().starts_with(b"."))
                        .filter(|e| e.file_type().is_ok_and(|t| if dirs { t.is_dir() } else { t.is_file() }))
                        .map(|e| e.path())
                })
                .into_iter()
                .flatten()
                .collect()
        };
        let mut files: Vec<PathBuf> =
            below(&self.top, true).iter().flat_map(|a| below(a, true)).flat_map(|b| below(&b, false)).collect();
        if files.len() <= 1 || files.len() as i64 <= max {
            return;
        }
        let remove = files.len() / 2 + 1;
        files.sort_by_cached_key(|f| fs::symlink_metadata(f).and_then(|m| m.modified()).ok());
        for f in &files[..remove] {
            let _ = fs::remove_file(f);
        }
        // Then the directories left empty, up to (not including) `top`.
        let mut dirs: Vec<PathBuf> = files[..remove].iter().filter_map(|f| f.parent().map(Path::to_path_buf)).collect();
        while !dirs.is_empty() {
            dirs.sort();
            dirs.dedup();
            dirs.retain(|d| *d != self.top && fs::remove_dir(d).is_ok());
            dirs = dirs.iter().filter_map(|d| d.parent().map(Path::to_path_buf)).collect();
        }
    }
}

/// One line, no tabs: an entry line is space-separated fields, then the message and an optional
/// tab-separated justification.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
        } else if let Some(next) = chars.next() {
            out.push(match next {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
        }
    }
    out
}

/// FileUtils.mkdir_p; Err holds the message of the EACCES/EROFS RuboCop rescues (other failures
/// skip saving quietly).
fn mkdir_p(dir: &Path) -> Result<(), Option<String>> {
    if dir.is_dir() {
        return Ok(());
    }
    let missing: Vec<&Path> = dir.ancestors().take_while(|d| !d.exists()).collect();
    for d in missing.into_iter().rev() {
        if let Err(e) = fs::create_dir(d)
            && !d.is_dir()
        {
            let what = match e.raw_os_error() {
                Some(libc::EACCES) => "Permission denied",
                Some(libc::EROFS) => "Read-only file system",
                _ => return Err(None),
            };
            return Err(Some(format!("{what} @ dir_s_mkdir - {}", d.display())));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        for m in ["plain", "a\\nb", "line\nbreak", "back\\\\slash\n", "tab\tand space "] {
            let escaped = escape(m);
            assert!(!escaped.contains('\n') && !escaped.contains('\t'));
            assert_eq!(unescape(&escaped), m);
        }
    }
}
