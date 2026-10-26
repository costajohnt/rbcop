//! rubocop-capybara's shared pieces: CssSelector, CssAttributesParser, CapybaraHelp, QueryMethods
//! and Visibility.

use crate::prelude::*;
use std::collections::HashMap;

/// A parsed attribute value (CssAttributesParser#normalize_value): `true`, `false`, none, or the
/// text with one pair of matching outer quotes removed.
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

/// CssAttributesParser#normalize_value
fn normalize_value(value: Option<&str>) -> AttrValue {
    // /\s+[is]\s*\z/i: a case-sensitivity flag
    static FLAG: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)[ \t\r\n\x0b\x0c]+[is][ \t\r\n\x0b\x0c]*\z").unwrap());
    match value {
        None => AttrValue::Nil,
        Some(v) if FLAG.is_match(v) => AttrValue::Nil,
        Some("true") => AttrValue::True,
        Some("false") => AttrValue::False,
        Some(v) => {
            let quoted = v.len() >= 2 && (v.starts_with('"') && v.ends_with('"') || v.starts_with('\'') && v.ends_with('\''));
            AttrValue::Str(if quoted { v[1..v.len() - 1].to_string() } else { v.to_string() })
        }
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
                // `@temp.split('=', 2)`
                let (key, value) = match temp.split_once('=') {
                    _ if temp.is_empty() => (None, None),
                    Some((k, v)) => (Some(k.to_string()), Some(v)),
                    None => (Some(temp.clone()), None),
                };
                let value = normalize_value(value);
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

/// `attrs[key]` (None when the key is absent).
pub fn attr<'a>(attrs: &'a [(Option<String>, AttrValue)], key: &str) -> Option<&'a AttrValue> {
    attrs.iter().find(|(k, _)| k.as_deref() == Some(key)).map(|(_, v)| v)
}

/// CssSelector.id?
pub fn is_id(selector: &str) -> bool {
    id(selector).is_some()
}

/// CssSelector.id: `selector[ID_PATTERN, 1]`.
pub fn id(selector: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A#((?:[A-Za-z0-9_-]|\\[.>,+~])+)(?:\.|\z)").unwrap());
    RE.captures(selector).map(|c| c[1].to_string())
}

/// CssSelector.classes: `scan(/(?<!\\)\.((?:\\.|[\w-])+)/)`, each with `\.` unescaped.
pub fn classes(selector: &str) -> Vec<String> {
    let chars: Vec<char> = selector.chars().collect();
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '.' || (i > 0 && chars[i - 1] == '\\') {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut name = String::new();
        loop {
            if chars.get(j) == Some(&'\\') && chars.get(j + 1).is_some_and(|&c| c != '\n') {
                name.push('\\');
                name.push(chars[j + 1]);
                j += 2;
            } else if chars.get(j).is_some_and(|&c| word(c)) {
                name.push(chars[j]);
                j += 1;
            } else {
                break;
            }
        }
        if name.is_empty() {
            i += 1;
        } else {
            out.push(name.replace("\\.", "."));
            i = j;
        }
    }
    out
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
pub fn specific_options(element: &str) -> &'static [&'static str] {
    match element {
        "button" => &["id", "class", "style", "disabled", "name", "value", "title", "type"],
        "link" => &["id", "class", "style", "href", "alt", "title", "download"],
        "table" => &["id", "class", "style", "cols", "rows"],
        "select" => &["id", "class", "style", "disabled", "name", "placeholder", "selected", "multiple"],
        "field" => &["id", "class", "style", "checked", "disabled", "name", "placeholder", "readonly", "type", "multiple"],
        _ => &[],
    }
}

/// CapybaraHelp.common_attributes?: `keys.difference(COMMON_OPTIONS).none?` (a nil key is falsy,
/// so it passes).
pub fn common_attributes(selector: &str) -> bool {
    attributes(selector).iter().all(|(k, _)| k.as_deref().is_none_or(|k| COMMON_OPTIONS.contains(&k)))
}

/// CapybaraHelp.replaceable_attributes?
pub fn replaceable_attributes(attrs: &[(Option<String>, AttrValue)]) -> bool {
    attrs.iter().all(|(_, v)| *v != AttrValue::Nil)
}

/// CapybaraHelp.replaceable_option?: `node` is the finder call (its symbols are searched for `:href`).
pub fn replaceable_option(cx: &Cx, node: &Node, locator: &str, element: &str) -> bool {
    let attrs = attributes(locator);
    if element == "link" && !(include_sym(cx, node, "href") || attr(&attrs, "href").is_some()) {
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
    "element",
];

/// `have_<matcher>`
pub fn positive_matcher(name: &str) -> bool {
    name.strip_prefix("have_").is_some_and(|m| CAPYBARA_MATCHERS.contains(&m))
}

/// `have_no_<matcher>`
pub fn negative_matcher(name: &str) -> bool {
    name.strip_prefix("have_no_").is_some_and(|m| CAPYBARA_MATCHERS.contains(&m))
}

/// CapybaraHelp::VISIBILITY_MATCHER_METHODS: `have_<e>` / `have_no_<e>`.
pub fn visibility_matcher_method(name: &str) -> bool {
    const ELEMENTS: &[&str] =
        &["button", "checked_field", "css", "field", "link", "select", "selector", "table", "unchecked_field", "xpath"];
    name.strip_prefix("have_no_").or_else(|| name.strip_prefix("have_")).is_some_and(|e| ELEMENTS.contains(&e))
}

pub const VISIBILITY_FALSE_MESSAGE: &str = "Use `:all` or `:hidden` instead of `false`.";
pub const VISIBILITY_TRUE_MESSAGE: &str = "Use `:visible` instead of `true`.";

/// `(call {nil? self} :page)` or `(call (const {nil? cbase} :Capybara) :current_session)`.
pub fn is_page_like(c: &ruby_prism::CallNode) -> bool {
    send_args(c).is_empty()
        && match method_name(c) {
            "page" => c.receiver().is_none_or(|r| r.as_self_node().is_some()),
            "current_session" => c.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Capybara")),
            _ => false,
        }
}

/// CapybaraHelp.capybara_receiver_pattern?: the Capybara DSL (no receiver, `self`, `page`,
/// `Capybara.current_session`) or a finder chain on it. `None` is a missing receiver. A call with a
/// block counts as its send (parser `(any_block #capybara_receiver_pattern? ...)`).
pub fn capybara_receiver_pattern(node: Option<&Node>) -> bool {
    const FINDERS: &[&str] =
        &["all", "ancestor", "find", "find_all", "find_button", "find_by_id", "find_field", "find_link", "first", "sibling"];
    let Some(node) = node else { return true };
    if node.as_self_node().is_some() {
        return true;
    }
    if let Some(p) = node.as_parentheses_node() {
        // (begin #capybara_receiver_pattern?): exactly one child
        let only = p.body().and_then(|b| match b.as_statements_node() {
            Some(s) if s.body().iter().count() == 1 => s.body().iter().next(),
            Some(_) => None,
            None => Some(b),
        });
        return only.is_some_and(|n| capybara_receiver_pattern(Some(&n)));
    }
    let Some(c) = node.as_call_node() else { return false };
    is_page_like(&c) || FINDERS.contains(&method_name(&c)) && capybara_receiver_pattern(c.receiver().as_ref())
}

/// CapybaraHelp.capybara_receiver?: the pattern, or a local variable whose visible preceding
/// assignment matches it. `ancestors` are the Prism ancestors of `node` (outermost first).
pub fn capybara_receiver(node: Option<&Node>, ancestors: &[&Node]) -> bool {
    capybara_receiver_pattern(node)
        || node.and_then(|n| local_variable_value(n, ancestors)).is_some_and(|v| capybara_receiver_pattern(Some(&v)))
}

/// Names a block's parameters bind (parser `arg optarg restarg kwarg kwoptarg kwrestarg blockarg
/// shadowarg`, nested destructuring included).
fn block_param_names(params: &Node) -> Vec<Vec<u8>> {
    struct V(Vec<Vec<u8>>);
    impl<'pr> ruby_prism::Visit<'pr> for V {
        fn visit_required_parameter_node(&mut self, n: &ruby_prism::RequiredParameterNode<'pr>) {
            self.0.push(n.name().as_slice().to_vec());
        }
        fn visit_optional_parameter_node(&mut self, n: &ruby_prism::OptionalParameterNode<'pr>) {
            self.0.push(n.name().as_slice().to_vec());
        }
        fn visit_rest_parameter_node(&mut self, n: &ruby_prism::RestParameterNode<'pr>) {
            if let Some(name) = n.name() {
                self.0.push(name.as_slice().to_vec());
            }
        }
        fn visit_required_keyword_parameter_node(&mut self, n: &ruby_prism::RequiredKeywordParameterNode<'pr>) {
            self.0.push(n.name().as_slice().to_vec());
        }
        fn visit_optional_keyword_parameter_node(&mut self, n: &ruby_prism::OptionalKeywordParameterNode<'pr>) {
            self.0.push(n.name().as_slice().to_vec());
        }
        fn visit_keyword_rest_parameter_node(&mut self, n: &ruby_prism::KeywordRestParameterNode<'pr>) {
            if let Some(name) = n.name() {
                self.0.push(name.as_slice().to_vec());
            }
        }
        fn visit_block_parameter_node(&mut self, n: &ruby_prism::BlockParameterNode<'pr>) {
            if let Some(name) = n.name() {
                self.0.push(name.as_slice().to_vec());
            }
        }
        fn visit_block_local_variable_node(&mut self, n: &ruby_prism::BlockLocalVariableNode<'pr>) {
            self.0.push(n.name().as_slice().to_vec());
        }
    }
    let mut v = V(Vec::new());
    ruby_prism::Visit::visit(&mut v, params);
    v.0
}

/// A parser `block` (not numblock/itblock) whose parameters bind `name`: a BlockNode or lambda.
fn shadows(node: &Node, name: &[u8]) -> bool {
    let params = if let Some(b) = node.as_block_node() {
        b.parameters()
    } else if let Some(l) = node.as_lambda_node() {
        l.parameters()
    } else {
        return false;
    };
    params.filter(|p| p.as_block_parameters_node().is_some()).is_some_and(|p| block_param_names(&p).iter().any(|n| n == name))
}

/// The statements of a parser `begin` among Prism nodes: a StatementsNode of two or more,
/// except directly in a `begin ... end` without rescue/ensure (parser `kwbegin`).
fn begin_children<'pr>(node: &Node<'pr>, parent: Option<&Node<'pr>>) -> Option<Vec<Node<'pr>>> {
    let s = node.as_statements_node()?;
    let body: Vec<Node> = s.body().iter().collect();
    if body.len() < 2 {
        return None;
    }
    let kwbegin = parent.and_then(|p| p.as_begin_node()).is_some_and(|b| {
        b.begin_keyword_loc().is_some()
            && b.rescue_clause().is_none()
            && b.ensure_clause().is_none()
            && b.statements().is_some_and(|st| st.location().start_offset() == s.location().start_offset())
    });
    (!kwbegin).then_some(body)
}

/// CapybaraHelp.local_variable_value: the value of the last `name = value` statement before the
/// local variable `node` in the scopes it can see. `ancestors` are its Prism ancestors.
pub fn local_variable_value<'pr>(node: &Node, ancestors: &[&Node<'pr>]) -> Option<Node<'pr>> {
    local_variable_value_in(node, ancestors).map(|(v, _, _)| v)
}

/// `local_variable_value` with where it was found: the value, the index in `ancestors` of the
/// statements holding the assignment, and the assignment (the value's ancestors are
/// `ancestors[..=i]` and the assignment).
pub fn local_variable_value_in<'pr>(node: &Node, ancestors: &[&Node<'pr>]) -> Option<(Node<'pr>, usize, Node<'pr>)> {
    let lvar = node.as_local_variable_read_node()?;
    let name = lvar.name().as_slice();
    let start = node.location().start_offset();
    for i in (0..ancestors.len()).rev() {
        let a = ancestors[i];
        match kind::of(a) {
            kind::DEF_NODE | kind::CLASS_NODE | kind::MODULE_NODE | kind::SINGLETON_CLASS_NODE => return None,
            kind::BLOCK_NODE | kind::LAMBDA_NODE if shadows(a, name) => return None,
            // a call owning a block is the block's send: the block is a parser ancestor too
            kind::CALL_NODE if a.as_call_node().unwrap().block().is_some_and(|b| shadows(&b, name)) => return None,
            _ => {}
        }
        if let Some(children) = begin_children(a, i.checked_sub(1).map(|p| ancestors[p])) {
            let found = children.into_iter().rev().find_map(|c| {
                let w = c.as_local_variable_write_node()?;
                (w.name().as_slice() == name && c.location().end_offset() <= start).then(|| (w.value(), c))
            });
            if let Some((value, write)) = found {
                return Some((value, i, write));
            }
        }
    }
    None
}

/// QueryMethods query types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryType {
    Finder,
    Collection,
    Selector,
    Match,
    Text,
    Action,
    Grouped,
}

/// QueryMethods::METHODS: each Capybara query method's type and selector (None: not applicable).
fn query_methods() -> &'static HashMap<String, (QueryType, Option<&'static str>)> {
    static METHODS: LazyLock<HashMap<String, (QueryType, Option<&'static str>)>> = LazyLock::new(|| {
        use QueryType::*;
        let mut m = HashMap::new();
        for (name, sel) in [
            ("find", "selector"),
            ("ancestor", "selector"),
            ("sibling", "selector"),
            ("find_button", "button"),
            ("find_by_id", "id"),
            ("find_field", "field"),
            ("find_link", "link"),
            ("within", "selector"),
        ] {
            m.insert(name.to_string(), (Finder, Some(sel)));
        }
        for name in ["all", "find_all", "first"] {
            m.insert(name.to_string(), (Collection, Some("selector")));
        }
        for selector in [
            "ancestor",
            "button",
            "checked_field",
            "css",
            "element",
            "field",
            "link",
            "select",
            "selector",
            "sibling",
            "table",
            "unchecked_field",
            "xpath",
        ] {
            let kind = match selector {
                "ancestor" | "sibling" => "selector",
                "checked_field" | "unchecked_field" => "field",
                s => s,
            };
            let mut prefixes = vec!["assert", "assert_no", "refute", "has", "has_no", "have", "have_no"];
            if selector != "element" {
                prefixes.extend(["must_have", "wont_have"]);
            }
            for prefix in prefixes {
                let q = if prefix.starts_with("has") { "?" } else { "" };
                m.insert(format!("{prefix}_{selector}{q}"), (Selector, Some(kind)));
            }
        }
        for s in ["css", "selector", "xpath"] {
            for name in [
                format!("assert_matches_{s}"),
                format!("assert_not_matches_{s}"),
                format!("refute_matches_{s}"),
                format!("matches_{s}?"),
                format!("not_matches_{s}?"),
                format!("match_{s}"),
                format!("not_match_{s}"),
                format!("must_match_{s}"),
                format!("wont_match_{s}"),
            ] {
                m.insert(name, (Match, Some(s)));
            }
        }
        for k in ["text", "content"] {
            for name in [
                format!("assert_{k}"),
                format!("assert_no_{k}"),
                format!("refute_{k}"),
                format!("has_{k}?"),
                format!("has_no_{k}?"),
                format!("have_{k}"),
                format!("have_no_{k}"),
                format!("must_have_{k}"),
                format!("wont_have_{k}"),
            ] {
                m.insert(name, (Text, None));
            }
        }
        for name in [
            "attach_file",
            "check",
            "choose",
            "click_button",
            "click_link",
            "click_link_or_button",
            "click_on",
            "fill_in",
            "select",
            "uncheck",
            "unselect",
        ] {
            m.insert(name.to_string(), (Action, None));
        }
        for prefix in ["assert", "have", "must_have"] {
            for count in ["all", "any", "none"] {
                m.insert(format!("{prefix}_{count}_of_selectors"), (Grouped, Some("selector")));
            }
        }
        m
    });
    &METHODS
}

/// QueryMethods.type
pub fn query_type(name: &str) -> Option<QueryType> {
    query_methods().get(name).map(|m| m.0)
}

/// QueryMethods.selector
pub fn query_selector_kind(name: &str) -> Option<&'static str> {
    query_methods().get(name).and_then(|m| m.1)
}

/// QueryMethods.built_in_selector?
pub fn built_in_selector(selector: Option<&str>) -> bool {
    const BUILT_IN: &[&str] = &[
        "button",
        "checkbox",
        "css",
        "datalist_input",
        "datalist_option",
        "element",
        "field",
        "fieldset",
        "file_field",
        "fillable_field",
        "frame",
        "id",
        "label",
        "link",
        "link_or_button",
        "option",
        "radio_button",
        "select",
        "table",
        "table_row",
        "xpath",
    ];
    selector.is_some_and(|s| BUILT_IN.contains(&s))
}

/// `cop_config['DefaultSelector'].to_s`
pub fn default_selector(cx: &Cx) -> String {
    cx.cfg.get("DefaultSelector").map(crate::config::validator::to_s).unwrap_or_default()
}

/// A `sym` node's value (parser `sym`, not `dsym`).
pub fn sym(cx: &Cx, node: &Node) -> Option<String> {
    if helpers::rspec::is_sym(cx, node) { helpers::rails::sym_value(node) } else { None }
}

/// Visibility#query_selector (also IgnoredQueryOption's, which differs only in `str_ok`: there a
/// string first argument does not keep two positional arguments on the default selector).
pub fn query_selector(cx: &Cx, call: &ruby_prism::CallNode, str_ok: bool) -> Option<String> {
    let args = send_args(call);
    let first = args.first();
    if let Some(s) = first.and_then(|f| sym(cx, f)) {
        return Some(s);
    }
    let positional = args
        .iter()
        .filter(|a| helpers::rails::hash_elements(a).is_none() && a.as_block_argument_node().is_none())
        .count();
    if positional > 1 && !(str_ok && first.is_some_and(helpers::rspec::is_str)) {
        return None;
    }
    Some(default_selector(cx))
}

/// Visibility#custom_filters?: `(hash <{(pair (sym :filter_set) _) (pair !sym _) kwsplat} ...>)`,
/// with `extra` keys counted like `filter_set`.
pub fn custom_filters(cx: &Cx, elements: &[Node], extra: &[&str]) -> bool {
    elements.iter().any(|e| match e.as_assoc_node() {
        Some(p) => match sym(cx, &p.key()) {
            Some(k) => k == "filter_set" || extra.contains(&k.as_str()),
            None => true,
        },
        None => true,
    })
}

/// Visibility#supported_visibility_query?
pub fn supported_visibility_query(cx: &Cx, call: &ruby_prism::CallNode, options: &Node) -> bool {
    if options.as_hash_node().is_some() || custom_filters(cx, &helpers::rails::hash_elements(options).unwrap_or_default(), &[]) {
        return false;
    }
    if query_selector_kind(method_name(call)) != Some("selector") {
        return true;
    }
    built_in_selector(query_selector(cx, call, true).as_deref())
}

/// Visibility#check_visibility: `visible: false` is reported, `visible: true` corrected to
/// `:visible` when `correct`.
pub fn check_visibility(cx: &mut Cx, pair: &ruby_prism::AssocNode, correct: bool) {
    let (s, e) = helpers::rspec::node_range(&pair.as_node());
    let value = pair.value();
    if value.as_false_node().is_some() {
        return cx.add_with(s, e, VISIBILITY_FALSE_MESSAGE, false);
    }
    let (vs, ve) = helpers::rspec::node_range(&value);
    cx.add_fix(s, e, VISIBILITY_TRUE_MESSAGE, |c| {
        if correct {
            c.replace(vs, ve, ":visible");
        }
    });
}

/// `Regexp.new(value).inspect` for CurrentPathExpectation / CurrentUrlExpectation, which give up
/// the correction for a value with `#{`, `#@` or `#$` or an invalid regexp.
pub enum RegexpExpr {
    Literal(String),
    /// RuboCop makes no correction.
    Uncorrectable,
    /// rbcop cannot tell: the correction is withheld (docs/known-gaps.md).
    Withheld,
}

/// For a value of printable ASCII (and tabs), `Regexp#inspect` is the value with unescaped `/`
/// escaped. Whether it compiles is what rbcop's Lint/Syntax checks for that regexp literal tell
/// (Prism's regexp errors, `regexp_parser`, then `onigmo_compile`); a value using something those
/// do not model,
/// or other characters, is `Withheld`.
pub fn regexp_from_string(value: &str) -> RegexpExpr {
    static INTERP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#[{@$]").unwrap());
    if INTERP.is_match(value) {
        return RegexpExpr::Uncorrectable;
    }
    if !value.bytes().all(|b| (0x20..0x7f).contains(&b) || b == b'\t') {
        return RegexpExpr::Withheld;
    }
    let literal = regexp_inspect(value);
    let source = &literal[1..literal.len() - 1];
    // unbalanced sets and groups: regexp_parser's errors are Onigmo's too
    if ruby_prism::parse(literal.as_bytes()).errors().next().is_some() || helpers::regexp_parser::parse(source, false).is_none() {
        return RegexpExpr::Uncorrectable;
    }
    match helpers::onigmo_compile::compiles(source) {
        Some(true) => RegexpExpr::Literal(literal),
        Some(false) => RegexpExpr::Uncorrectable,
        None => RegexpExpr::Withheld,
    }
}

/// `/value/` with unescaped `/` escaped.
fn regexp_inspect(value: &str) -> String {
    let mut out = String::from("/");
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push('\\');
                out.extend(chars.next());
            }
            '/' => out.push_str("\\/"),
            c => out.push(c),
        }
    }
    out.push('/');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_selector() {
        assert_eq!(id("#foo.bar > a"), Some("foo".into()));
        assert_eq!(id("#foo > a"), None);
        assert_eq!(id("#foo"), Some("foo".into()));
        assert_eq!(id(r"#foo\.bar"), Some(r"foo\.bar".into()));
        assert_eq!(classes("#a.b.c-d"), vec!["b", "c-d"]);
        assert_eq!(classes(r".a\.b\.c"), vec!["a.b.c"]);
        assert_eq!(classes(r"x\.y.z"), vec!["z"]);
        assert_eq!(pseudo_classes("a[x=':y']:checked"), vec!["checked"]);
        assert!(multiple_selectors("a b"));
        assert!(!multiple_selectors("a:not(b c)"));
        assert_eq!(
            attributes("[id='a'][disabled][x=true][y=\"b'c\"][z=d i][q=a=b]"),
            vec![
                (Some("id".into()), AttrValue::Str("a".into())),
                (Some("disabled".into()), AttrValue::Nil),
                (Some("x".into()), AttrValue::True),
                (Some("y".into()), AttrValue::Str("b'c".into())),
                (Some("z".into()), AttrValue::Nil),
                (Some("q".into()), AttrValue::Str("a=b".into())),
            ]
        );
        assert_eq!(attributes("[]"), vec![(None, AttrValue::Nil)]);
    }

    #[test]
    fn regexp_text() {
        // as rubocop-capybara 3.1.0 with RuboCop 1.91.0: invalid regexps get no correction
        let lit = |v: &str| match regexp_from_string(v) {
            RegexpExpr::Literal(l) => Some(l),
            _ => None,
        };
        assert_eq!(lit(r"/users/\d+/edit").as_deref(), Some(r"/\/users\/\d+\/edit/"));
        assert_eq!(lit(r"a/b\/c").as_deref(), Some(r"/a\/b\/c/"));
        assert_eq!(lit("a**").as_deref(), Some("/a**/"));
        for bad in ["*a", "[", "[]", "(", "a)", "[z-a]", "a\\", "(?<n>a)\\1", "/tags/#{x}", "/tags/#@x"] {
            assert!(matches!(regexp_from_string(bad), RegexpExpr::Uncorrectable), "{bad}");
        }
        for unknown in ["é", r"\p{Foo}"] {
            assert!(matches!(regexp_from_string(unknown), RegexpExpr::Withheld), "{unknown}");
        }
    }

    #[test]
    fn query_methods_table() {
        assert_eq!(query_type("find"), Some(QueryType::Finder));
        assert_eq!(query_selector_kind("has_checked_field?"), Some("field"));
        assert_eq!(query_type("must_have_element"), None);
        assert_eq!(query_type("select"), Some(QueryType::Action));
        assert_eq!(query_selector_kind("have_no_text"), None);
        assert_eq!(query_type("have_any_of_selectors"), Some(QueryType::Grouped));
    }
}
