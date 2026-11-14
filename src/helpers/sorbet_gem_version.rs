//! rubocop-sorbet's GemVersionAnnotationHelper: `# @version ...` comments in RBI files.

use crate::prelude::*;
use crate::source::Source;

const VERSION_PREFIX: &str = "# @version";

/// gem_version_annotations: the comments starting with `# @version`.
pub fn gem_version_annotations<'a>(src: &'a Source) -> impl Iterator<Item = &'a Comment> + 'a {
    src.comments.iter().filter(|c| src.slice(c.start, c.end).starts_with(VERSION_PREFIX))
}

/// gem_versions: `text.delete_prefix(VERSION_PREFIX).split(/, ?/).map(&:strip)` (Ruby's split
/// drops trailing empty fields).
pub fn gem_versions(text: &str) -> Vec<String> {
    let rest = text.strip_prefix(VERSION_PREFIX).unwrap_or(text);
    let mut parts: Vec<&str> = Vec::new();
    let mut from = 0;
    while let Some(i) = rest[from..].find(',') {
        parts.push(&rest[from..from + i]);
        from += i + 1;
        if rest[from..].starts_with(' ') {
            from += 1;
        }
    }
    parts.push(&rest[from..]);
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts.into_iter().map(|p| helpers::ruby_string::strip(p).to_string()).collect()
}
