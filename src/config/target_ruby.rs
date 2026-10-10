//! RuboCop's TargetRuby: which Ruby version a project targets, from the first source that knows.

use super::value::{Map, Value};
use regex::Regex;
use std::path::{Path, PathBuf};

pub const KNOWN_RUBIES: &[f64] = &[2.0, 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 3.0, 3.1, 3.2, 3.3, 3.4, 4.0, 4.1];
pub const DEFAULT_VERSION: f64 = 2.7;

/// Ruby's String#to_f on a version-ish string ("3.4.1" is 3.4).
fn to_f(s: &str) -> Option<f64> {
    let re = Regex::new(r"^\s*(\d+(?:\.\d+)?)").unwrap();
    re.captures(s).and_then(|c| c[1].parse().ok())
}

/// The version and, for messages, which source supplied it.
pub fn detect(all_cops: &Map, base_dir: &Path, project_root: Option<&Path>) -> f64 {
    if let Some(v) = std::env::var("RUBOCOP_TARGET_RUBY_VERSION").ok().and_then(|s| to_f(&s)) {
        return v;
    }
    match all_cops.get("TargetRubyVersion") {
        Some(Value::Float(f)) => return *f,
        Some(Value::Int(i)) => return *i as f64,
        Some(Value::Str(s)) => {
            if let Some(v) = to_f(s) {
                return v;
            }
        }
        _ => {}
    }
    if let Some(v) = gemspec_version(base_dir, project_root) {
        return v;
    }
    let file_sources: [(&str, &str); 3] = [
        (".ruby-version", r"\A(?:ruby-)?(\d+\.\d+)"),
        ("mise.toml", r#"(?m)^ruby = ["'](\d+\.\d+)"#),
        (".tool-versions", r"(?m)^(?:ruby )(\d+\.\d+)"),
    ];
    for (name, pattern) in file_sources {
        if let Some(file) = find_file_upwards(name, base_dir, project_root) {
            let text = std::fs::read_to_string(file).unwrap_or_default();
            // A file that exists but does not match yields no version, so the next source is tried.
            if let Some(v) = Regex::new(pattern).unwrap().captures(&text).and_then(|c| c[1].parse().ok()) {
                return v;
            }
        }
    }
    // Config#bundler_lock_file_path needs a loaded_path, which RuboCop always has: the default
    // configuration is loaded from default.yml (base dir = pwd).
    for name in ["Gemfile.lock", "gems.locked"] {
        if let Some(file) = find_file_upwards(name, base_dir, project_root) {
            if let Some(v) = lockfile_version(&std::fs::read_to_string(file).unwrap_or_default()) {
                return v;
            }
            break;
        }
    }
    DEFAULT_VERSION
}

/// FileFinder#find_file_upwards with stop_dir = project root (inclusive).
fn find_file_upwards(name: &str, start: &Path, stop: Option<&Path>) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if Some(dir) == stop {
            break;
        }
    }
    None
}

fn lockfile_version(text: &str) -> Option<f64> {
    let mut in_ruby = false;
    let section = Regex::new(r"^\s*RUBY\s*VERSION\s*$").unwrap();
    let version = Regex::new(r"^\s*ruby\s+(\d+\.\d+)[p.\d]*\s*$").unwrap();
    for line in text.lines() {
        in_ruby |= section.is_match(line);
        if in_ruby && let Some(c) = version.captures(line) {
            return c[1].parse().ok();
        }
    }
    None
}

/// TargetRuby::GemspecFile: the only *.gemspec in the nearest directory that has exactly one.
fn gemspec_version(base_dir: &Path, project_root: Option<&Path>) -> Option<f64> {
    let mut file = None;
    for dir in base_dir.ancestors() {
        let specs: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "gemspec") && p.is_file())
            .collect();
        if specs.len() == 1 {
            file = specs.into_iter().next();
            break;
        }
        if Some(dir) == project_root {
            break;
        }
    }
    let text = std::fs::read(file?).ok()?;
    let requirements = required_ruby_version(&text)?;
    KNOWN_RUBIES
        .iter()
        .copied()
        .find(|v| requirements.iter().all(|r| r.satisfied_by(&[(*v as u64), ((v * 10.0).round() as u64) % 10, 99])))
}

/// Strings on the right of `x.required_ruby_version = ...` (str, [str...], Gem::Requirement.new(str...)).
fn required_ruby_version(source: &[u8]) -> Option<Vec<Requirement>> {
    use ruby_prism::Visit;
    struct Finder(Option<Vec<String>>);
    fn strs(nodes: Vec<ruby_prism::Node>) -> Option<Vec<String>> {
        nodes
            .into_iter()
            .map(|n| {
                let n = match n.as_call_node() {
                    Some(c) if c.name().as_slice() == b"freeze" && c.arguments().is_none() => c.receiver()?,
                    _ => n,
                };
                n.as_string_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
            })
            .collect()
    }
    impl<'pr> Visit<'pr> for Finder {
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
            if self.0.is_none() && node.name().as_slice() == b"required_ruby_version=" {
                let rhs = node.arguments().and_then(|a| a.arguments().iter().next());
                self.0 = rhs.and_then(|rhs| {
                    if let Some(s) = rhs.as_string_node() {
                        return Some(vec![String::from_utf8_lossy(s.unescaped()).into_owned()]);
                    }
                    if let Some(a) = rhs.as_array_node() {
                        return strs(a.elements().iter().collect());
                    }
                    let c = rhs.as_call_node()?;
                    let recv = c.receiver()?;
                    let recv_src = String::from_utf8_lossy(recv.location().as_slice()).into_owned();
                    if c.name().as_slice() != b"new" || recv_src.trim_start_matches("::") != "Gem::Requirement" {
                        return None;
                    }
                    let args: Vec<_> = c.arguments()?.arguments().iter().collect();
                    match args.first().and_then(|a| a.as_array_node()) {
                        Some(a) if args.len() == 1 => strs(a.elements().iter().collect()),
                        _ => strs(args),
                    }
                });
            }
            ruby_prism::visit_call_node(self, node);
        }
    }
    let result = ruby_prism::parse(source);
    if result.errors().next().is_some() {
        return None;
    }
    let mut finder = Finder(None);
    finder.visit(&result.node());
    finder.0.map(|v| v.iter().filter_map(|s| Requirement::parse(s)).collect())
}

/// One `Gem::Requirement` clause such as `>= 3.1` or `~> 3.2.0`.
struct Requirement {
    op: String,
    version: Vec<u64>,
}

impl Requirement {
    fn parse(s: &str) -> Option<Requirement> {
        let re = Regex::new(r"^\s*(=|!=|>=|<=|>|<|~>)?\s*(\d+(?:\.\d+)*)").unwrap();
        let c = re.captures(s)?;
        Some(Requirement {
            op: c.get(1).map_or("=", |m| m.as_str()).to_string(),
            version: c[2].split('.').filter_map(|p| p.parse().ok()).collect(),
        })
    }

    fn satisfied_by(&self, v: &[u64]) -> bool {
        let cmp = compare(v, &self.version);
        match self.op.as_str() {
            "=" => cmp == std::cmp::Ordering::Equal,
            "!=" => cmp != std::cmp::Ordering::Equal,
            ">" => cmp.is_gt(),
            "<" => cmp.is_lt(),
            ">=" => cmp.is_ge(),
            "<=" => cmp.is_le(),
            _ => {
                // ~> x.y.z means >= x.y.z and < x.(y+1)
                let mut upper: Vec<u64> = self.version[..self.version.len().saturating_sub(1).max(1)].to_vec();
                if let Some(last) = upper.last_mut() {
                    *last += 1;
                }
                cmp.is_ge() && compare(v, &upper).is_lt()
            }
        }
    }
}

/// Gem::Version ordering (missing segments are zero).
fn compare(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)))
        .find(|o| o.is_ne())
        .unwrap_or(std::cmp::Ordering::Equal)
}

/// The Prism grammar version RuboCop uses for a target (Prism for 3.3+, else the parser gem,
/// which rbcop approximates with Prism's oldest grammar).
pub fn prism_version(target: f64) -> &'static str {
    if target >= 4.05 {
        "4.1"
    } else if target >= 3.45 {
        "4.0"
    } else if target >= 3.35 {
        "3.4"
    } else {
        "3.3"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gem_requirements() {
        let first = |reqs: &[&str]| {
            let reqs: Vec<Requirement> = reqs.iter().filter_map(|s| Requirement::parse(s)).collect();
            KNOWN_RUBIES
                .iter()
                .copied()
                .find(|v| reqs.iter().all(|r| r.satisfied_by(&[(*v as u64), ((v * 10.0).round() as u64) % 10, 99])))
        };
        assert_eq!(first(&[">= 3.1.0"]), Some(3.1));
        assert_eq!(first(&["~> 3.2"]), Some(3.2));
        assert_eq!(first(&[">= 2.7", "< 4"]), Some(2.7));
        assert_eq!(first(&["> 3.3"]), Some(3.3));
        assert_eq!(first(&["3.0.0"]), None);
        assert_eq!(lockfile_version("GEM\n\nRUBY VERSION\n   ruby 3.3.4p94\n"), Some(3.3));
    }
}
