use crate::helpers::sorbet_gem_version::{gem_version_annotations, gem_versions};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ValidGemVersionAnnotations";
pub static COP: ValidGemVersionAnnotations = ValidGemVersionAnnotations;
pub struct ValidGemVersionAnnotations;

const VALID_OPERATORS: &[&str] = &["=", "!=", ">", ">=", "<", "<=", "~>"];

/// Gem::Version::ANCHORED_VERSION_PATTERN on a blank-free word (the version is optional).
static VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A(?:[0-9]+(?:\.[0-9a-zA-Z]+)*(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?)?\z").unwrap()
});

/// valid_version?: `operator, version = strip.split(" ")`; a missing version is valid
/// (`Gem::Version.correct?(nil)`).
fn valid_version(v: &str) -> bool {
    let mut parts = v.split(|c: char| helpers::ruby_string::is_space(c)).filter(|p| !p.is_empty());
    let Some(op) = parts.next() else { return false };
    VALID_OPERATORS.contains(&op) && parts.next().is_none_or(|ver| VERSION.is_match(ver))
}

impl Cop for ValidGemVersionAnnotations {
    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        for c in gem_version_annotations(src) {
            let versions = gem_versions(src.slice(c.start, c.end));
            if versions.is_empty() {
                cx.add(c.start, c.end, "Invalid gem version(s) detected: empty version");
                break;
            }
            let invalid: Vec<&str> = versions.iter().map(String::as_str).filter(|v| !valid_version(v)).collect();
            if !invalid.is_empty() {
                cx.add(c.start, c.end, format!("Invalid gem version(s) detected: {}", invalid.join(", ")));
            }
        }
    }
}
