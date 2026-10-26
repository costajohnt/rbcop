use crate::prelude::*;
use helpers::capybara::{self as capy, QueryType};
use helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use ruby_prism::{AssocNode, CallNode};

pub const NAME: &str = "Capybara/IgnoredQueryOption";
pub static COP: IgnoredQueryOption = IgnoredQueryOption;
pub struct IgnoredQueryOption;

const COUNT_OPTIONS: &[&str] = &["count", "minimum", "maximum", "between"];

/// explicit_selector: a `sym` first argument's value.
fn explicit_selector(cx: &Cx, call: &CallNode) -> Option<String> {
    send_args(call).first().and_then(|a| capy::sym(cx, a))
}

/// custom_selector?
fn custom_selector(cx: &Cx, call: &CallNode) -> bool {
    capy::query_selector_kind(method_name(call)) == Some("selector")
        && !capy::built_in_selector(capy::query_selector(cx, call, false).as_deref())
}

/// rubocop-ast `truthy_literal?`
fn truthy_literal(n: &Node) -> bool {
    matches!(
        kind::of(n),
        kind::STRING_NODE
        | kind::INTERPOLATED_STRING_NODE
        | kind::X_STRING_NODE
        | kind::INTERPOLATED_X_STRING_NODE
        | kind::SOURCE_FILE_NODE
        | kind::INTEGER_NODE
        | kind::SOURCE_LINE_NODE
        | kind::FLOAT_NODE
        | kind::SYMBOL_NODE
        | kind::INTERPOLATED_SYMBOL_NODE
        | kind::ARRAY_NODE
        | kind::HASH_NODE
        | kind::KEYWORD_HASH_NODE
        | kind::REGULAR_EXPRESSION_NODE
        | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE
        | kind::TRUE_NODE
        | kind::RANGE_NODE
        | kind::IMAGINARY_NODE
        | kind::RATIONAL_NODE
    )
}

/// removable_value?: `{basic_literal? (range {int nil?} {int nil?})}`
fn removable_value(cx: &Cx, n: &Node) -> bool {
    if let Some(r) = n.as_range_node() {
        let end_ok = |e: Option<Node>| e.is_none_or(|e| e.as_integer_node().is_some());
        return end_ok(r.left()) && end_ok(r.right());
    }
    match kind::of(n) {
        kind::STRING_NODE => !helpers::rspec::is_dstr(n),
        kind::SYMBOL_NODE => !helpers::rspec::is_dsym(cx, n),
        kind::SOURCE_FILE_NODE
        | kind::INTEGER_NODE
        | kind::SOURCE_LINE_NODE
        | kind::FLOAT_NODE
        | kind::TRUE_NODE
        | kind::FALSE_NODE
        | kind::NIL_NODE
        | kind::IMAGINARY_NODE
        | kind::RATIONAL_NODE => true,
        _ => false,
    }
}

impl Cop for IgnoredQueryOption {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let Some(qtype) = capy::query_type(name) else { return };
        if !matches!(qtype, QueryType::Finder | QueryType::Collection | QueryType::Selector | QueryType::Match | QueryType::Text) {
            return;
        }
        // (call #capybara_receiver? _ <$(hash ...) ...>)
        let mut chain: Vec<&Node> = cx.ancestors.iter().collect();
        chain.push(node);
        if !capy::capybara_receiver(call.receiver().as_ref(), &chain) {
            return;
        }
        let args = send_args(&call);
        let Some(options) = args.iter().find(|a| helpers::rails::hash_elements(a).is_some()) else { return };
        let elements = helpers::rails::hash_elements(options).unwrap();
        if options.as_hash_node().is_some() || capy::custom_filters(cx, &elements, &["session_options"]) {
            return;
        }
        // every element is a pair with a `sym` key now
        let pairs: Vec<(String, AssocNode)> =
            elements.iter().map(|e| e.as_assoc_node().unwrap()).map(|p| (capy::sym(cx, &p.key()).unwrap(), p)).collect();
        let range = |p: &AssocNode| helpers::rspec::node_range(&p.as_node());
        let effective = |key: &str| pairs.iter().rev().find(|(k, _)| k == key).map(|(_, p)| p);
        let same = |a: &AssocNode, b: &AssocNode| {
            let (ra, rb) = (range(a), range(b));
            ra == rb || helpers::node_equality::shape(cx.src, ra.0, ra.1) == helpers::node_equality::shape(cx.src, rb.0, rb.1)
        };
        let selector = capy::query_selector_kind(name);
        let explicit = explicit_selector(cx, &call);
        let mut issues: Vec<(&AssocNode, String, bool)> = Vec::new();
        for (option, pair) in &pairs {
            let option = option.as_str();
            let value = pair.value();
            let issue = if qtype == QueryType::Finder && COUNT_OPTIONS.contains(&option) && !custom_selector(cx, &call) {
                // find_count_issue
                Some((format!("`{option}` has no effect when passed to `{name}`."), false))
            } else if option == "exact" && (selector == Some("css") || selector == Some("selector") && explicit.as_deref() == Some("css")) {
                Some(("`exact` has no effect on CSS queries.".to_string(), true))
            } else if option == "exact_text"
                && qtype != QueryType::Text
                && !custom_selector(cx, &call)
                && effective("exact_text").is_some_and(|e| same(e, pair))
                && (value.as_true_node().is_some() || value.as_false_node().is_some())
                && effective("text").is_some_and(|t| {
                    let v = t.value();
                    v.as_regular_expression_node().is_some() || v.as_interpolated_regular_expression_node().is_some()
                })
            {
                Some(("`exact_text` has no effect when `text` is a regexp.".to_string(), true))
            } else if option == "match"
                && qtype == QueryType::Collection
                && capy::built_in_selector(explicit.as_deref())
                && capy::sym(cx, &value).is_some_and(|v| matches!(v.as_str(), "first" | "smart" | "prefer_exact" | "one"))
            {
                Some((format!("`match` has no effect when passed to `{name}`."), true))
            } else if qtype != QueryType::Match
                && (selector != Some("selector") || capy::built_in_selector(explicit.as_deref()))
                && option != "count"
                && COUNT_OPTIONS.contains(&option)
                && effective("count").is_some_and(|c| truthy_literal(&c.value()))
            {
                // shadowed_count_issue
                Some((format!("`{option}` has no effect when `count` is specified."), true))
            } else {
                None
            };
            if let Some((msg, correctable)) = issue {
                issues.push((pair, msg, correctable));
            }
        }
        if issues.is_empty() {
            return;
        }
        // register_issues: the first correctable, removable issue is corrected, unless the call has comments
        let (s, e) = send_range(&call);
        let (first_line, last_line) = (cx.src.line_of(s), cx.src.line_of(e));
        let comments = cx.src.comments.iter().any(|c| (first_line..=last_line).contains(&cx.src.line_of(c.start)));
        let removable = |pair: &AssocNode| {
            let v = pair.value();
            let key = pairs.iter().find(|(_, o)| range(o) == range(pair)).map(|(k, _)| k);
            removable_value(cx, &v)
                && !(helpers::rspec::is_str(&v) && is_heredoc(&v))
                && pairs.iter().filter(|(k, _)| Some(k) == key).count() == 1
        };
        let correction = issues.iter().find(|(p, _, c)| *c && removable(p) && !comments).map(|(p, _, _)| range(p));
        // remove_option
        let first = args.first().is_some_and(|a| a.location().start_offset() == options.location().start_offset())
            && elements.first().is_some_and(|f| correction.is_some_and(|c| helpers::rspec::node_range(f) == c));
        let removal = correction.map(|(ps, pe)| {
            let side = if first { Side::Right } else { Side::Left };
            let opts = Surrounding { side, ..Default::default() };
            let (rs, re) = range_with_surrounding_space(cx.src.text, ps, pe, opts);
            let (rs, re) = helpers::rails::range_with_surrounding_comma(cx.src.text, rs, re, !first, first);
            range_with_surrounding_space(cx.src.text, rs, re, opts)
        });
        for (pair, msg, _) in issues {
            let (ps, pe) = range(pair);
            let edit = removal.filter(|_| correction == Some((ps, pe)));
            cx.add_fix(ps, pe, msg, |c| {
                if let Some((rs, re)) = edit {
                    c.remove(rs, re);
                }
            });
        }
    }
}
