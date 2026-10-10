//! `File.fnmatch?(pattern, path, File::FNM_PATHNAME | File::FNM_EXTGLOB [| File::FNM_DOTMATCH])`,
//! ported from Ruby's dir.c so Include/Exclude patterns match exactly what RuboCop matches.

/// Does `path` match `pattern` under Ruby's fnmatch with FNM_PATHNAME and FNM_EXTGLOB?
pub fn fnmatch(pattern: &str, path: &str, dotmatch: bool) -> bool {
    expand_braces(pattern).iter().any(|p| fnmatch_path(p.as_bytes(), path.as_bytes(), dotmatch))
}

/// Ruby's `glob?` test in PathUtil: does the string contain glob metacharacters?
pub fn is_glob(s: &str) -> bool {
    s.contains(['*', '{', '[', '?'])
}

/// `Dir.glob(pattern)` for files, sorted: relative patterns are matched below `cwd` and come back
/// relative to it.
// ponytail: walks below the pattern's literal directory prefix and filters with `fnmatch`; does not
// follow symlinked directories.
pub fn dir_glob(pattern: &str, cwd: &std::path::Path) -> Vec<String> {
    let segments: Vec<&str> = pattern.split('/').collect();
    let literal = segments.iter().take_while(|s| !is_glob(s)).count().min(segments.len() - 1);
    let prefix = segments[..literal].join("/");
    let depth = if pattern.contains("**") { usize::MAX } else { segments.len() - literal };
    let root =
        if pattern.starts_with('/') { std::path::PathBuf::from(format!("/{prefix}")) } else { cwd.join(&prefix) };
    let mut out = Vec::new();
    let mut stack = vec![(root, prefix, 0)];
    while let Some((dir, shown, level)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if shown.is_empty() && !pattern.starts_with('/') { name } else { format!("{shown}/{name}") };
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if level + 1 < depth {
                    stack.push((entry.path(), rel, level + 1));
                }
            } else if entry.path().is_file() && fnmatch(pattern, &rel, false) {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// dir.c `fnmatch` with FNM_PATHNAME: segment by segment, `**/` matching any number of segments.
fn fnmatch_path(p: &[u8], s: &[u8], dotmatch: bool) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut retry: Option<(usize, usize)> = None; // (pattern after `**/`, string segment to resume from)
    loop {
        if p[pi..].starts_with(b"**/") {
            while p[pi..].starts_with(b"**/") {
                pi += 3;
                while p.get(pi) == Some(&b'/') {
                    pi += 1;
                }
            }
            retry = Some((pi, si));
        }
        let pend = segment_end(p, pi);
        let send = segment_end(s, si);
        if fnmatch_segment(&p[pi..pend], &s[si..send], dotmatch) {
            match (pend < p.len(), send < s.len()) {
                (true, true) => {
                    pi = pend + 1;
                    si = send + 1;
                    continue;
                }
                (false, false) => return true,
                _ => {}
            }
        }
        // Mismatch: let the last `**/` swallow one more (non-hidden) directory.
        let Some((ptmp, stmp)) = retry else { return false };
        if !dotmatch && s.get(stmp) == Some(&b'.') {
            return false;
        }
        let next = segment_end(s, stmp);
        if next >= s.len() {
            return false;
        }
        pi = ptmp;
        si = next + 1;
        retry = Some((ptmp, si));
    }
}

fn segment_end(b: &[u8], from: usize) -> usize {
    b[from..].iter().position(|&c| c == b'/').map_or(b.len(), |i| from + i)
}

/// dir.c `fnmatch_helper` for one path segment (no `/` on either side).
fn fnmatch_segment(p: &[u8], s: &[u8], dotmatch: bool) -> bool {
    let unescaped_first = if p.first() == Some(&b'\\') { p.get(1) } else { p.first() };
    if !dotmatch && s.first() == Some(&b'.') && unescaped_first != Some(&b'.') {
        return false;
    }
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi < p.len() {
            match p[pi] {
                b'*' => {
                    while p.get(pi) == Some(&b'*') {
                        pi += 1;
                    }
                    if pi == p.len() {
                        return true;
                    }
                    star = Some((pi, si));
                    continue;
                }
                b'?' if si < s.len() => {
                    pi += 1;
                    si += char_len(s, si);
                    continue;
                }
                b'[' if si < s.len() => {
                    if let Some((matched, next)) = bracket(p, pi, s, si)
                        && matched
                    {
                        pi = next;
                        si += char_len(s, si);
                        continue;
                    }
                }
                c => {
                    let (pc, plen) = if c == b'\\' && pi + 1 < p.len() { (p[pi + 1], 2) } else { (c, 1) };
                    if si < s.len() && s[si] == pc && c != b'?' && c != b'[' {
                        pi += plen;
                        si += 1;
                        continue;
                    }
                }
            }
        } else if si == s.len() {
            return true;
        }
        // Mismatch: backtrack to the last `*`, which takes one more character.
        let Some((sp, ss)) = star else { return false };
        if ss >= s.len() {
            return false;
        }
        let ss = ss + char_len(s, ss);
        star = Some((sp, ss));
        pi = sp;
        si = ss;
    }
}

fn char_len(s: &[u8], i: usize) -> usize {
    match s[i] {
        b if b < 0x80 => 1,
        b if b >= 0xF0 => 4,
        b if b >= 0xE0 => 3,
        _ => 2,
    }
    .min(s.len() - i)
}

/// `[...]` at p[pi]: Some((matched, index after `]`)), or None if the bracket is unterminated
/// (then `[` is treated as a literal by the caller's fallthrough).
fn bracket(p: &[u8], pi: usize, s: &[u8], si: usize) -> Option<(bool, usize)> {
    let c = decode(s, si);
    let mut i = pi + 1;
    let negate = matches!(p.get(i), Some(b'!' | b'^'));
    if negate {
        i += 1;
    }
    let mut ok = false;
    let mut first = true;
    while i < p.len() && (p[i] != b']' || first) {
        first = false;
        let (lo, n) = class_char(p, i)?;
        i += n;
        if p.get(i) == Some(&b'-') && p.get(i + 1).is_some_and(|&b| b != b']') {
            let (hi, n) = class_char(p, i + 1)?;
            i += 1 + n;
            ok |= lo <= c && c <= hi;
        } else {
            ok |= lo == c;
        }
    }
    if i >= p.len() {
        return None;
    }
    Some((ok != negate, i + 1))
}

fn class_char(p: &[u8], i: usize) -> Option<(char, usize)> {
    if p[i] == b'\\' && i + 1 < p.len() {
        let c = decode(p, i + 1);
        return Some((c, 1 + c.len_utf8()));
    }
    let c = decode(p, i);
    Some((c, c.len_utf8()))
}

fn decode(b: &[u8], i: usize) -> char {
    std::str::from_utf8(&b[i..i + char_len(b, i)]).ok().and_then(|s| s.chars().next()).unwrap_or('\u{fffd}')
}

/// Ruby's brace expansion for FNM_EXTGLOB: `a{b,c{d,e}}` becomes `ab`, `acd`, `ace`.
fn expand_braces(pattern: &str) -> Vec<String> {
    let b = pattern.as_bytes();
    let mut depth = 0;
    let mut open = None;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'{' => {
                if depth == 0 {
                    open = Some(i);
                }
                depth += 1;
            }
            b'}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let start = open.unwrap();
                    let (head, tail) = (&pattern[..start], &pattern[i + 1..]);
                    let mut out = Vec::new();
                    for alt in split_top_level(&pattern[start + 1..i]) {
                        out.extend(expand_braces(&format!("{head}{alt}{tail}")));
                    }
                    return out;
                }
            }
            _ => {}
        }
        i += 1;
    }
    vec![pattern.to_string()]
}

fn split_top_level(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let (mut parts, mut depth, mut last, mut i) = (Vec::new(), 0, 0, 0);
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&s[last..i]);
                last = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&s[last..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::fnmatch;

    // Expected values checked against Ruby 4.0 `File.fnmatch?` with FNM_PATHNAME | FNM_EXTGLOB.
    #[test]
    fn matches_like_ruby() {
        let cases: &[(&str, &str, bool)] = &[
            ("**/*.rb", "a.rb", true),
            ("**/*.rb", "lib/a.rb", true),
            ("**/*.rb", "lib/deep/er/a.rb", true),
            ("**/*.rb", ".hidden/a.rb", false),
            ("**/*.rb", "lib/.a.rb", false),
            ("vendor/**/*", "vendor/a/b.rb", true),
            ("vendor/**/*", "vendor/b.rb", true),
            ("vendor/**", "vendor/a/b.rb", false),
            ("vendor/**", "vendor/b.rb", true),
            ("*.rb", "lib/a.rb", false),
            ("lib/*.rb", "lib/a.rb", true),
            ("**/Gemfile", "Gemfile", true),
            ("**/Gemfile", "x/y/Gemfile", true),
            ("**/*.{rb,rake}", "x/t.rake", true),
            ("{app,lib}/**/*.rb", "lib/x/y.rb", true),
            ("spec/[ab]*.rb", "spec/b1.rb", true),
            ("spec/[!ab]*.rb", "spec/b1.rb", false),
            ("a?c", "abc", true),
            ("a?c", "a/c", false),
            ("**/tmp/**/*", "x/tmp/y/z.rb", true),
            ("/abs/**/*.rb", "/abs/q/r.rb", true),
            ("**/*Fastfile", "fastlane/Fastfile", true),
        ];
        for &(p, s, want) in cases {
            assert_eq!(fnmatch(p, s, false), want, "fnmatch({p:?}, {s:?})");
        }
        assert!(fnmatch("**/*.rb", "lib/.a.rb", true));
        assert!(fnmatch("**/*", ".github/x.rb", true));
    }
}
