//! RuboCop::UnifiedDiff (RuboCop 1.91.0 lib/rubocop/unified_diff.rb): the `--diff` output, a
//! unified diff of a file's source before and after autocorrection. Ported step for step (Myers'
//! algorithm, its tie-breaking and the fallback past MAX_EDIT_DISTANCE), since it is not meant to
//! match `diff -u` and only reproducing it gives RuboCop's hunks.

use std::collections::HashMap;

const CONTEXT_LINES: usize = 3;
const NO_NEWLINE_MARKER: &str = "\\ No newline at end of file\n";
/// Past this many edits the changed region is reported as a single replacement.
const MAX_EDIT_DISTANCE: i64 = 400;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Equal,
    Delete,
    Insert,
}

type Script<'a> = Vec<(Kind, &'a str)>;

/// The diff of `old` to `new` with `--- a/<path>` / `+++ b/<path>` headers, or "" when the sources
/// are identical.
pub fn unified_diff(path: &str, old: &str, new: &str) -> String {
    // String#lines: each line keeps its "\n".
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    if old_lines == new_lines {
        return String::new();
    }
    let script = edit_script(&old_lines, &new_lines);
    let hunks = hunks_for(&script);
    if hunks.is_empty() {
        return String::new();
    }
    format!("--- a/{path}\n+++ b/{path}\n{}", hunks.concat())
}

fn edit_script<'a>(old: &[&'a str], new: &[&'a str]) -> Script<'a> {
    let mut prefix = 0;
    while prefix < old.len() && Some(&old[prefix]) == new.get(prefix) {
        prefix += 1;
    }
    let mut suffix = 0;
    let max = old.len().min(new.len()) - prefix;
    while suffix < max && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix] {
        suffix += 1;
    }
    let (old_last, new_last) = (old.len() - suffix, new.len() - suffix);
    let mut script: Script = old[..prefix].iter().map(|&l| (Kind::Equal, l)).collect();
    let (from, to) = (&old[prefix..old_last], &new[prefix..new_last]);
    match (Myers { from, to }).script() {
        Some(s) => script.extend(s),
        None => {
            script.extend(from.iter().map(|&l| (Kind::Delete, l)));
            script.extend(to.iter().map(|&l| (Kind::Insert, l)));
        }
    }
    script.extend(old[old_last..].iter().map(|&l| (Kind::Equal, l)));
    script
}

/// Hash.new(0): a missing diagonal reads 0.
type Furthest = HashMap<i64, i64>;

fn at(f: &Furthest, k: i64) -> i64 {
    f.get(&k).copied().unwrap_or(0)
}

struct Myers<'s, 'a> {
    from: &'s [&'a str],
    to: &'s [&'a str],
}

impl<'a> Myers<'_, 'a> {
    fn script(&self) -> Option<Script<'a>> {
        let mut trace: Vec<Furthest> = Vec::new();
        let mut furthest = Furthest::new();
        let max_distance = ((self.from.len() + self.to.len()) as i64).min(MAX_EDIT_DISTANCE);
        for distance in 0..=max_distance {
            trace.push(furthest.clone());
            let mut diagonal = -distance;
            while diagonal <= distance {
                let x = self.advance(&furthest, diagonal, distance);
                furthest.insert(diagonal, x);
                if x >= self.from.len() as i64 && x - diagonal >= self.to.len() as i64 {
                    return Some(self.backtrack(&trace));
                }
                diagonal += 2;
            }
        }
        None
    }

    fn advance(&self, f: &Furthest, diagonal: i64, distance: i64) -> i64 {
        let previous = previous_diagonal(f, diagonal, distance);
        let old_index = if previous > diagonal { at(f, previous) } else { at(f, previous) + 1 };
        self.follow_diagonal(old_index, old_index - diagonal)
    }

    fn follow_diagonal(&self, mut old_index: i64, mut new_index: i64) -> i64 {
        while old_index < self.from.len() as i64
            && new_index < self.to.len() as i64
            && new_index >= 0
            && self.from[old_index as usize] == self.to[new_index as usize]
        {
            old_index += 1;
            new_index += 1;
        }
        old_index
    }

    fn backtrack(&self, trace: &[Furthest]) -> Script<'a> {
        // Built back to front (Ruby unshifts), reversed at the end.
        let mut script = Vec::new();
        let (mut x, mut y) = (self.from.len() as i64, self.to.len() as i64);
        for (distance, f) in trace.iter().enumerate().rev() {
            let previous = previous_diagonal(f, x - y, distance as i64);
            let (px, py) = (at(f, previous), at(f, previous) - previous);
            while x > px && y > py {
                x -= 1;
                y -= 1;
                script.push((Kind::Equal, self.from[x as usize]));
            }
            if distance == 0 {
                continue;
            }
            if x == px {
                y -= 1;
                script.push((Kind::Insert, self.to[y as usize]));
            } else {
                x -= 1;
                script.push((Kind::Delete, self.from[x as usize]));
            }
        }
        script.reverse();
        script
    }
}

/// The path reaching this diagonal comes from the one above (an insertion) or below (a deletion),
/// whichever reached further.
fn previous_diagonal(f: &Furthest, diagonal: i64, distance: i64) -> i64 {
    if diagonal == -distance || (diagonal != distance && at(f, diagonal - 1) < at(f, diagonal + 1)) {
        diagonal + 1
    } else {
        diagonal - 1
    }
}

fn hunks_for(script: &Script) -> Vec<String> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (index, _) in script.iter().enumerate().filter(|(_, (k, _))| *k != Kind::Equal) {
        let first = index.saturating_sub(CONTEXT_LINES);
        let last = (index + CONTEXT_LINES).min(script.len() - 1);
        match ranges.last_mut() {
            Some(r) if first <= r.1 + 1 => r.1 = last,
            _ => ranges.push((first, last)),
        }
    }
    ranges.into_iter().map(|(first, last)| hunk(script, first, last)).collect()
}

fn hunk(script: &Script, first: usize, last: usize) -> String {
    let (mut old_start, mut new_start) = (1, 1);
    for (kind, _) in &script[..first] {
        old_start += usize::from(*kind != Kind::Insert);
        new_start += usize::from(*kind != Kind::Delete);
    }
    let body = &script[first..=last];
    let old_count = body.iter().filter(|(k, _)| *k != Kind::Insert).count();
    let new_count = body.iter().filter(|(k, _)| *k != Kind::Delete).count();
    let mut out = format!("@@ -{} +{} @@\n", position(old_start, old_count), position(new_start, new_count));
    for &(kind, line) in body {
        out.push(match kind {
            Kind::Equal => ' ',
            Kind::Delete => '-',
            Kind::Insert => '+',
        });
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push('\n');
            out.push_str(NO_NEWLINE_MARKER);
        }
    }
    out
}

/// An empty side is anchored to the line it follows; a single line drops the count.
fn position(start: usize, count: usize) -> String {
    match count {
        0 => format!("{},0", start - 1),
        1 => start.to_string(),
        _ => format!("{start},{count}"),
    }
}

#[cfg(test)]
mod tests {
    use super::unified_diff;

    #[test]
    fn hunks_context_and_markers() {
        assert_eq!(unified_diff("a.rb", "x\n", "x\n"), "");
        assert_eq!(
            unified_diff("a.rb", "a\nb\n", "a\nc"),
            "--- a/a.rb\n+++ b/a.rb\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n\\ No newline at end of file\n"
        );
        assert_eq!(unified_diff("a.rb", "a\n", "x\na\n"), "--- a/a.rb\n+++ b/a.rb\n@@ -1 +1,2 @@\n+x\n a\n");
        // An empty side is anchored to the line before it.
        assert_eq!(unified_diff("a.rb", "a\nb\n", "a\n"), "--- a/a.rb\n+++ b/a.rb\n@@ -1,2 +1 @@\n a\n-b\n");
        assert_eq!(unified_diff("a.rb", "a\n", ""), "--- a/a.rb\n+++ b/a.rb\n@@ -1 +0,0 @@\n-a\n");
        // Changes more than 2 * CONTEXT_LINES apart are separate hunks.
        let old: String = (1..=10).map(|i| format!("{i}\n")).collect();
        let new = old.replace("1\n", "one\n").replace("10\n", "ten\n");
        let d = unified_diff("p", &old, &new);
        assert_eq!(d.matches("@@ -").count(), 2, "{d}");
    }
}
