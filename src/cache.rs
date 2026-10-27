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

/// The entry layout, part of the build digest: entries of another layout are never read.
/// 2: each offense line carries its corrections (RuboCop 1.92.1's Offense#corrections).
const FORMAT: &[u8] = b"format 2";

/// One saved offense's corrections: `-`, or `begin,end,hex(replacement)` joined by `;`. None when
/// malformed or outside `0..=len`, or not on char boundaries of `text`.
fn parse_edits(field: &str, text: &str) -> Option<Option<Vec<(usize, usize, String)>>> {
    if field == "-" {
        return Some(None);
    }
    let mut edits = Vec::new();
    for e in field.split(';') {
        let mut f = e.split(',');
        let (b, e, hex) = (f.next()?.parse().ok()?, f.next()?.parse().ok()?, f.next()?);
        if f.next().is_some() || b > e || !text.is_char_boundary(b) || !text.is_char_boundary(e) {
            return None;
        }
        if hex.len() % 2 != 0 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        edits.push((b, e, String::from_utf8(bytes).ok()?));
    }
    Some(Some(edits))
}

fn encode_edits(edits: &Option<Vec<(usize, usize, String)>>) -> String {
    match edits {
        Some(edits) if !edits.is_empty() => edits
            .iter()
            .map(|(b, e, r)| format!("{b},{e},{}", r.bytes().map(|x| format!("{x:02x}")).collect::<String>()))
            .collect::<Vec<_>>()
            .join(";"),
        _ => "-".to_string(),
    }
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
        let build = digest(&[env!("CARGO_PKG_VERSION").as_bytes(), &exe.finish().to_le_bytes(), FORMAT]);
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

    /// The file a report is saved in.
    pub fn entry(&self, key: &str) -> PathBuf {
        self.dir.join(key)
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
        // runner::inspect_bytes keeps the source as read for an encoding error, else with CRLF
        // normalised; the saved length tells which.
        let raw = String::from_utf8_lossy(bytes).into_owned();
        let text = if raw.len() == text_len { raw } else { raw.replace("\r\n", "\n") };
        if text.len() != text_len {
            return None;
        }
        let mut offenses = Vec::new();
        for line in lines.filter(|l| !l.is_empty()) {
            let mut f = line.splitn(7, ' ');
            let (start, end, severity, flags, cop, edits, rest) =
                (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
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
                // A malformed or out-of-range correction makes the entry a miss.
                edits: parse_edits(edits, &text)?,
            });
        }
        Some(FileReport { offenses, text, warnings: Vec::new(), abort: None, diffs: Vec::new() })
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
        let mut out = format!("{}\n", report.text.len());
        for o in &report.offenses {
            let flags = format!("{}{}{}", u8::from(o.correctable), u8::from(o.global), u8::from(o.disabled));
            let mut message = escape(&o.message);
            if let Some(j) = &o.justification {
                message = format!("{message}\t{}", escape(j));
            }
            let edits = encode_edits(&o.edits);
            out.push_str(&format!("{} {} {} {flags} {} {edits} {message}\n", o.start, o.end, o.severity.name(), o.cop));
        }
        // Written aside and renamed, so a parallel run never reads half an entry. The temporary
        // name is unpredictable and created fresh (never through an existing file or symlink);
        // any failure skips saving, with ResultCache#write_cache_file's warning for EACCES, EPERM and
        // EROFS. Without `write` the file is only created and removed again, for that warning.
        let n = self.saved.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let random = std::collections::hash_map::RandomState::new().hash_one(n);
        let tmp = self.dir.join(format!("{key}_{}_{random:016x}", std::process::id()));
        let dest = self.dir.join(key);
        let opened = fs::OpenOptions::new().write(true).create_new(true).custom_flags(libc::O_NOFOLLOW).open(&tmp);
        let written = match opened {
            Err(e) => Err((e, format!("rb_sysopen - {}", tmp.display()))),
            Ok(_) if !write => Ok(()),
            Ok(mut f) => {
                f.write_all(out.as_bytes()).map_err(|e| (e, format!("rb_sysopen - {}", tmp.display()))).and_then(|()| {
                    fs::rename(&tmp, &dest)
                        .map_err(|e| (e, format!("rb_file_s_rename - ({}, {})", tmp.display(), dest.display())))
                })
            }
        };
        if !write || written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        if let Err((e, at)) = written
            && let Some(what) = errno_message(&e)
        {
            eprintln!("Couldn't write cache file. Continuing without cache.\n  {what} @ {at}");
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

    /// ResultCache.cleanup: past MaxFilesInCache entries, remove the oldest half (and one more);
    /// `verbose` (`-d`) says so.
    pub fn cleanup(&self, verbose: bool) {
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
        if verbose {
            println!("Removing the {remove} oldest files from {}", self.top.display());
        }
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

/// The strerror of the errors ResultCache rescues (EACCES, EPERM, EROFS).
fn errno_message(e: &std::io::Error) -> Option<&'static str> {
    match e.raw_os_error() {
        Some(libc::EACCES) => Some("Permission denied"),
        Some(libc::EPERM) => Some("Operation not permitted"),
        Some(libc::EROFS) => Some("Read-only file system"),
        _ => None,
    }
}

/// FileUtils.mkdir_p; Err holds the message of the EACCES/EPERM/EROFS RuboCop rescues (other
/// failures skip saving quietly).
fn mkdir_p(dir: &Path) -> Result<(), Option<String>> {
    if dir.is_dir() {
        return Ok(());
    }
    let missing: Vec<&Path> = dir.ancestors().take_while(|d| !d.exists()).collect();
    for d in missing.into_iter().rev() {
        if let Err(e) = fs::create_dir(d)
            && !d.is_dir()
        {
            let Some(what) = errno_message(&e) else { return Err(None) };
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

    #[test]
    fn corrections_round_trip_and_bad_entries_miss() {
        let root = std::env::temp_dir().join(format!("rbcop-cache-edits-{}", std::process::id()));
        let names: HashSet<&'static str> = ["Style/X"].into_iter().collect();
        let cache = Cache::new(&root, "ctx", true, None, names).unwrap();
        let text = "é = 1 \n";
        let offense = Offense {
            start: 6,
            end: 7,
            cop: "Style/X",
            message: "m".into(),
            severity: Severity::Convention,
            correctable: true,
            global: false,
            corrected: false,
            source: None,
            disabled: false,
            justification: None,
            todo: false,
            edits: Some(vec![(6, 7, String::new()), (0, 0, "a;b,\n".into())]),
        };
        let report = FileReport {
            offenses: vec![offense],
            text: text.into(),
            warnings: Vec::new(),
            abort: None,
            diffs: Vec::new(),
        };
        cache.save("k", &report, true);
        let loaded = cache.load("k", text.as_bytes()).unwrap();
        assert_eq!(loaded.offenses[0].edits, report.offenses[0].edits);
        let entry = |body: &str| {
            fs::write(cache.entry("k"), format!("{}\n{body}\n", text.len())).unwrap();
            cache.load("k", text.as_bytes()).map(|r| r.offenses.len())
        };
        assert_eq!(entry("6 7 convention 100 Style/X - m"), Some(1));
        // Truncated or garbage lines, bad hex, and edits outside the source or inside a character.
        for bad in [
            "6 7 convention 100 Style/X",
            "garbage",
            "6 7 convention 100 Style/X 6,7,zz m",
            "6 7 convention 100 Style/X 6,7,0 m",
            "6 7 convention 100 Style/X 6,99, m",
            "6 7 convention 100 Style/X 7,6, m",
            "6 7 convention 100 Style/X 1,1, m",
            "6 7 convention 100 Style/X 6,7 m",
        ] {
            assert_eq!(entry(bad), None, "{bad}");
        }
        let _ = fs::remove_dir_all(&root);
    }
}
