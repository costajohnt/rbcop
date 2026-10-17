//! rubocop-capybara's shared pieces: CssSelector, CssAttributesParser and CapybaraHelp.

use crate::prelude::*;

/// A parsed attribute value (CssAttributesParser#normalize_value): `true`, `false`, none, or the
/// text with its quotes removed and wrapped in single quotes.
#[derive(Clone, Debug, PartialEq)]
pub enum AttrValue {
    True,
    False,
    Nil,
    Str(String),
}

impl AttrValue {
    /// Ruby's `to_s` of the value.
    pub fn to_s(&self) -> String {
        match self {
            AttrValue::True => "true".into(),
            AttrValue::False => "false".into(),
            AttrValue::Nil => String::new(),
            AttrValue::Str(s) => s.clone(),
        }
    }

    /// Ruby truthiness.
    pub fn truthy(&self) -> bool {
        !matches!(self, AttrValue::False | AttrValue::Nil)
    }
}

/// CssSelector.attributes: `[key=value]` pairs in order of first appearance (a key may be nil).
pub fn attributes(selector: &str) -> Vec<(Option<String>, AttrValue)> {
    let mut results: Vec<(Option<String>, AttrValue)> = Vec::new();
    let mut inside = false;
    let mut temp = String::new();
    let mut count: i64 = 0;
    for ch in selector.chars() {
        if ch == '[' {
            count += 1;
            if inside {
                temp.push('[');
            } else {
                inside = true;
            }
        } else if ch == ']' {
            count -= 1;
            if count == 0 {
                inside = false;
                let parts = helpers::rails::ruby_split(&temp, '=');
                let key = parts.first().map(|s| s.to_string());
                let value = match parts.get(1).copied() {
                    Some("true") => AttrValue::True,
                    Some("false") => AttrValue::False,
                    None => AttrValue::Nil,
                    Some(v) => AttrValue::Str(format!("'{}'", v.replace(['"', '\''], ""))),
                };
                match results.iter_mut().find(|(k, _)| *k == key) {
                    Some(r) => r.1 = value,
                    None => results.push((key, value)),
                }
                temp.clear();
            } else {
                temp.push(']');
            }
        } else if inside {
            temp.push(ch);
        }
    }
    results
}

/// CssSelector.id?
pub fn is_id(selector: &str) -> bool {
    selector.starts_with('#')
}

/// CssSelector.id: the selector without `#`s and without what follows a combinator or class.
pub fn id(selector: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\\]([>,+~.].*)").unwrap());
    if !is_id(selector) {
        return None;
    }
    let tail: String = RE.captures_iter(selector).map(|c| c[1].to_string()).collect();
    let stripped = selector.replace('#', "");
    Some(if tail.is_empty() { stripped } else { stripped.replace(&tail, "") })
}

/// CssSelector.classes
pub fn classes(selector: &str) -> Vec<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.([A-Za-z0-9_-]*)").unwrap());
    RE.captures_iter(selector).map(|c| c[1].to_string()).collect()
}

/// CssSelector.attribute?
pub fn is_attribute(selector: &str) -> bool {
    selector.starts_with('[')
}

/// CssSelector.pseudo_classes
pub fn pseudo_classes(selector: &str) -> Vec<String> {
    static ATTR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[.*?\]").unwrap());
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":([^:]*)").unwrap());
    let ignored = ATTR.replace_all(selector, "");
    RE.captures_iter(&ignored).map(|c| c[1].to_string()).collect()
}

/// CssSelector.multiple_selectors?
pub fn multiple_selectors(selector: &str) -> bool {
    static NORM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\\[>,+~]|\(.*\))").unwrap());
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ >,+~]").unwrap());
    RE.is_match(&NORM.replace_all(selector, ""))
}

const COMMON_OPTIONS: &[&str] = &["id", "class", "style"];

/// CapybaraHelp::SPECIFIC_OPTIONS
fn specific_options(element: &str) -> &'static [&'static str] {
    match element {
        "button" => &["id", "class", "style", "disabled", "name", "value", "title", "type"],
        "link" => &["id", "class", "style", "href", "alt", "title", "download"],
        "table" => &["id", "class", "style", "cols", "rows"],
        "select" => &["id", "class", "style", "disabled", "name", "placeholder", "selected", "multiple"],
        "field" => &["id", "class", "style", "checked", "disabled", "name", "placeholder", "readonly", "type", "multiple"],
        _ => &[],
    }
}

/// CapybaraHelp.common_attributes?
pub fn common_attributes(selector: &str) -> bool {
    attributes(selector).iter().all(|(k, _)| k.as_deref().is_some_and(|k| COMMON_OPTIONS.contains(&k)))
}

/// CapybaraHelp.replaceable_attributes?
pub fn replaceable_attributes(attrs: &[(Option<String>, AttrValue)]) -> bool {
    attrs.iter().all(|(_, v)| *v != AttrValue::Nil)
}

/// CapybaraHelp.replaceable_option?: `node` is the finder call (its symbols are searched for `:href`).
pub fn replaceable_option(cx: &Cx, node: &Node, locator: &str, element: &str) -> bool {
    let attrs = attributes(locator);
    let has = |k: &str| attrs.iter().any(|(a, _)| a.as_deref() == Some(k));
    if element == "link" && !(include_sym(cx, node, "href") || has("href")) {
        return false;
    }
    let allowed = specific_options(element);
    attrs.iter().all(|(k, _)| k.as_deref().is_some_and(|k| allowed.contains(&k)))
}

/// CapybaraHelp.include_option?: a `sym` descendant with this value.
fn include_sym(cx: &Cx, node: &Node, value: &str) -> bool {
    struct V<'a>(f64, &'a str, bool);
    impl<'pr> ruby_prism::Visit<'pr> for V<'_> {
        fn visit_symbol_node(&mut self, n: &ruby_prism::SymbolNode<'pr>) {
            let node = n.as_node();
            self.2 |= !is_parser_dsym(&node, self.0) && n.unescaped() == self.1.as_bytes();
        }
    }
    let mut v = V(cx.src.target_ruby, value, false);
    visit_send(node, &mut v);
    v.2
}

/// Visit a node as parser's `send` (a call's receiver and arguments, not its block).
pub fn visit_send<'pr>(node: &Node<'pr>, v: &mut impl ruby_prism::Visit<'pr>) {
    match node.as_call_node() {
        Some(c) => {
            if let Some(r) = c.receiver() {
                v.visit(&r);
            }
            if let Some(a) = c.arguments() {
                v.visit_arguments_node(&a);
            }
            if let Some(b) = c.block().filter(|b| b.as_block_argument_node().is_some()) {
                v.visit(&b);
            }
        }
        None => v.visit(node),
    }
}

/// CapybaraHelp.replaceable_pseudo_classes?
pub fn replaceable_pseudo_classes(locator: &str) -> bool {
    static NOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"not\(.*?\)").unwrap());
    pseudo_classes(locator).iter().all(|p| match p.as_str() {
        "not()" => NOT
            .find_iter(locator)
            .all(|m| attributes(m.as_str()).iter().all(|(_, v)| matches!(v, AttrValue::True | AttrValue::False))),
        "disabled" | "enabled" | "checked" | "unchecked" => true,
        _ => false,
    })
}

/// CapybaraHelp::CAPYBARA_MATCHERS
pub const CAPYBARA_MATCHERS: &[&str] = &[
    "selector",
    "css",
    "xpath",
    "text",
    "title",
    "current_path",
    "link",
    "button",
    "field",
    "checked_field",
    "unchecked_field",
    "select",
    "table",
    "sibling",
    "ancestor",
    "content",
];

/// `have_<matcher>`
pub fn positive_matcher(name: &str) -> bool {
    name.strip_prefix("have_").is_some_and(|m| CAPYBARA_MATCHERS.contains(&m))
}

/// `have_no_<matcher>`
pub fn negative_matcher(name: &str) -> bool {
    name.strip_prefix("have_no_").is_some_and(|m| CAPYBARA_MATCHERS.contains(&m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_selector() {
        assert_eq!(id("#foo.bar > a"), Some("foo".into()));
        assert_eq!(id("#foo"), Some("foo".into()));
        assert_eq!(classes("#a.b.c-d"), vec!["b", "c-d"]);
        assert_eq!(pseudo_classes("a[x=':y']:checked"), vec!["checked"]);
        assert!(multiple_selectors("a b"));
        assert!(!multiple_selectors("a:not(b c)"));
        assert_eq!(
            attributes("[id='a'][disabled][x=true]"),
            vec![
                (Some("id".into()), AttrValue::Str("'a'".into())),
                (Some("disabled".into()), AttrValue::Nil),
                (Some("x".into()), AttrValue::True)
            ]
        );
    }
}
