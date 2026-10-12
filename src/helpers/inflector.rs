//! ActiveSupport::Inflector with the default English rules (activesupport 8.1,
//! active_support/inflections.rb): what rubocop-rails cops use (`underscore`, `foreign_key`,
//! `tableize`, `pluralize`, `singularize`, `demodulize`).

use regex::Regex;
use std::sync::LazyLock;

type Rules = Vec<(Regex, &'static str)>;

/// `irregular(singular, plural)`: four rules each for plurals and singulars (first letters differ
/// in case only when they differ, so the case-sensitive branch is the one used for these words).
fn irregulars() -> Vec<(String, String)> {
    [("person", "people"), ("man", "men"), ("child", "children"), ("sex", "sexes"), ("move", "moves"), ("zombie", "zombies")]
        .iter()
        .map(|(s, p)| (s.to_string(), p.to_string()))
        .collect()
}

static PLURALS: LazyLock<Rules> = LazyLock::new(|| {
    let mut defs: Vec<(String, String)> = [
        ("$", "s"),
        ("(?i)s$", "s"),
        ("(?i)^(ax|test)is$", "${1}es"),
        ("(?i)(octop|vir)us$", "${1}i"),
        ("(?i)(octop|vir)i$", "${1}i"),
        ("(?i)(alias|status)$", "${1}es"),
        ("(?i)(bu)s$", "${1}ses"),
        ("(?i)(buffal|tomat)o$", "${1}oes"),
        ("(?i)([ti])um$", "${1}a"),
        ("(?i)([ti])a$", "${1}a"),
        ("(?i)sis$", "ses"),
        ("(?i)(?:([^f])fe|([lr])f)$", "${1}${2}ves"),
        ("(?i)(hive)$", "${1}s"),
        ("(?i)([^aeiouy]|qu)y$", "${1}ies"),
        ("(?i)(x|ch|ss|sh)$", "${1}es"),
        ("(?i)(matr|vert|ind)(?:ix|ex)$", "${1}ices"),
        ("(?i)^(m|l)ouse$", "${1}ice"),
        ("(?i)^(m|l)ice$", "${1}ice"),
        ("(?i)^(ox)$", "${1}en"),
        ("(?i)^(oxen)$", "${1}"),
        ("(?i)(quiz)$", "${1}zes"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    for (s, p) in irregulars() {
        let (s0, srest, prest) = (&s[..1], &s[1..], &p[1..]);
        defs.push((format!("(?i)({s0}){srest}$"), format!("${{1}}{prest}")));
        defs.push((format!("(?i)({s0}){prest}$"), format!("${{1}}{prest}")));
    }
    defs.iter().rev().map(|(re, rep)| (Regex::new(re).unwrap(), &*String::leak(rep.clone()))).collect()
});

static SINGULARS: LazyLock<Rules> = LazyLock::new(|| {
    let mut defs: Vec<(String, String)> = [
        ("(?i)s$", ""),
        ("(?i)(ss)$", "${1}"),
        ("(?i)(n)ews$", "${1}ews"),
        ("(?i)([ti])a$", "${1}um"),
        ("(?i)((a)naly|(b)a|(d)iagno|(p)arenthe|(p)rogno|(s)ynop|(t)he)(sis|ses)$", "${1}sis"),
        ("(?i)(^analy)(sis|ses)$", "${1}sis"),
        ("(?i)([^f])ves$", "${1}fe"),
        ("(?i)(hive)s$", "${1}"),
        ("(?i)(tive)s$", "${1}"),
        ("(?i)([lr])ves$", "${1}f"),
        ("(?i)([^aeiouy]|qu)ies$", "${1}y"),
        ("(?i)(s)eries$", "${1}eries"),
        ("(?i)(m)ovies$", "${1}ovie"),
        ("(?i)(x|ch|ss|sh)es$", "${1}"),
        ("(?i)^(m|l)ice$", "${1}ouse"),
        ("(?i)(bus)(es)?$", "${1}"),
        ("(?i)(o)es$", "${1}"),
        ("(?i)(shoe)s$", "${1}"),
        ("(?i)(cris|test)(is|es)$", "${1}is"),
        ("(?i)^(a)x[ie]s$", "${1}xis"),
        ("(?i)(octop|vir)(us|i)$", "${1}us"),
        ("(?i)(alias|status)(es)?$", "${1}"),
        ("(?i)^(ox)en", "${1}"),
        ("(?i)(vert|ind)ices$", "${1}ex"),
        ("(?i)(matr)ices$", "${1}ix"),
        ("(?i)(quiz)zes$", "${1}"),
        ("(?i)(database)s$", "${1}"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    for (s, p) in irregulars() {
        let (s0, srest, prest) = (&s[..1], &s[1..], &p[1..]);
        defs.push((format!("(?i)({s0}){srest}$"), format!("${{1}}{srest}")));
        defs.push((format!("(?i)({s0}){prest}$"), format!("${{1}}{srest}")));
    }
    defs.iter().rev().map(|(re, rep)| (Regex::new(re).unwrap(), &*String::leak(rep.clone()))).collect()
});

static UNCOUNTABLE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    ["equipment", "information", "rice", "money", "species", "series", "fish", "sheep", "jeans", "police"]
        .iter()
        .map(|w| Regex::new(&format!(r"(?i)\b{w}\z")).unwrap())
        .collect()
});

fn apply(word: &str, rules: &Rules) -> String {
    if word.is_empty() || UNCOUNTABLE.iter().any(|r| r.is_match(word)) {
        return word.to_string();
    }
    for (re, rep) in rules {
        if re.is_match(word) {
            return re.replace(word, *rep).into_owned();
        }
    }
    word.to_string()
}

pub fn pluralize(word: &str) -> String {
    apply(word, &PLURALS)
}

pub fn singularize(word: &str) -> String {
    apply(word, &SINGULARS)
}

/// `underscore` (no acronyms defined).
pub fn underscore(word: &str) -> String {
    if !word.bytes().any(|b| b.is_ascii_uppercase() || b == b'-') && !word.contains("::") {
        return word.to_string();
    }
    let w = word.replace("::", "/");
    // gsub(/(?<=[A-Z])(?=[A-Z][a-z])|(?<=[a-z\d])(?=[A-Z])/, "_")
    let chars: Vec<char> = w.chars().collect();
    let mut out = String::with_capacity(w.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            if (prev.is_ascii_uppercase() && c.is_ascii_uppercase() && next_lower)
                || ((prev.is_ascii_lowercase() || prev.is_ascii_digit()) && c.is_ascii_uppercase())
            {
                out.push('_');
            }
        }
        out.push(c);
    }
    out.replace('-', "_").to_lowercase()
}

pub fn demodulize(path: &str) -> &str {
    path.rfind("::").map_or(path, |i| &path[i + 2..])
}

/// `foreign_key(class_name)`: `underscore(demodulize(class_name)) + "_id"`.
pub fn foreign_key(class_name: &str) -> String {
    format!("{}_id", underscore(demodulize(class_name)))
}

/// `tableize(class_name)`: `pluralize(underscore(class_name))`.
pub fn tableize(class_name: &str) -> String {
    pluralize(&underscore(class_name))
}

#[test]
fn inflections() {
    assert_eq!(pluralize("person"), "people");
    assert_eq!(pluralize("user"), "users");
    assert_eq!(pluralize("category"), "categories");
    assert_eq!(pluralize("status"), "statuses");
    assert_eq!(pluralize("sheep"), "sheep");
    assert_eq!(pluralize("admin_mouse"), "admin_mouses");
    assert_eq!(singularize("people"), "person");
    assert_eq!(singularize("statuses"), "status");
    assert_eq!(underscore("Admin::HTMLPage"), "admin/html_page");
    assert_eq!(tableize("Admin_UserProfile"), "admin_user_profiles");
    assert_eq!(foreign_key("Foo::Bar"), "bar_id");
}
