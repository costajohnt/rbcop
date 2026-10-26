use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Capybara/RSpec/PredicateMatcher";
pub static COP: PredicateMatcher = PredicateMatcher;
pub struct PredicateMatcher;

const SUFFIXES: &[&str] = &["css", "selector", "style", "xpath"];
const BUILT_IN_MATCHERS: &[&str] =
    &["be_truthy", "be_falsey", "be_falsy", "have_attributes", "have_received", "be_between", "be_within"];

/// args_loc: from the end of the selector to the end of the send.
fn args_source<'a>(src: &'a str, call: &CallNode) -> &'a str {
    let sel = call.message_loc().unwrap().end_offset();
    &src[sel..send_range(call).1]
}

/// The send of `node` or of a block call `node` (parser `(block $(send ...) ...)`).
fn send_or_block_send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let c = node.as_call_node()?;
    (!is_csend(&c)).then_some(c)
}

/// actual_requires_parentheses?: the predicate appended to `actual` would bind to part of it.
fn requires_parentheses(cx: &Cx, actual: &Node) -> bool {
    if actual.as_parentheses_node().is_some() {
        return false;
    }
    if let Some(c) = actual.as_call_node() {
        if helpers::rspec::block_of(&c).is_some() {
            // a parser `block`, not a call: unsafe
            return true;
        }
        let name = method_name(&c);
        return name != "[]"
            && (helpers::node_types::is_operator_method(name) || !send_args(&c).is_empty() && c.opening_loc().is_none());
    }
    // SAFE_RECEIVER_TYPES
    let safe = match kind::of(actual) {
        kind::ARRAY_NODE
        | kind::IMAGINARY_NODE
        | kind::CONSTANT_READ_NODE
        | kind::CONSTANT_PATH_NODE
        | kind::CLASS_VARIABLE_READ_NODE
        | kind::INTERPOLATED_STRING_NODE
        | kind::STRING_NODE
        | kind::SOURCE_FILE_NODE
        | kind::FALSE_NODE
        | kind::FLOAT_NODE
        | kind::GLOBAL_VARIABLE_READ_NODE
        | kind::HASH_NODE
        | kind::KEYWORD_HASH_NODE
        | kind::INTEGER_NODE
        | kind::SOURCE_LINE_NODE
        | kind::INSTANCE_VARIABLE_READ_NODE
        | kind::LOCAL_VARIABLE_READ_NODE
        | kind::IT_LOCAL_VARIABLE_READ_NODE
        | kind::NIL_NODE
        | kind::RATIONAL_NODE
        | kind::REGULAR_EXPRESSION_NODE
        | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE
        | kind::SELF_NODE
        | kind::TRUE_NODE
        | kind::X_STRING_NODE
        | kind::INTERPOLATED_X_STRING_NODE => true,
        kind::SYMBOL_NODE => !helpers::rspec::is_dsym(cx, actual),
        _ => false,
    };
    !safe
}

impl Cop for PredicateMatcher {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let to = method_name(&call);
        if is_csend(&call) || !matches!(to, "to" | "to_not" | "not_to") {
            return;
        }
        // (send (send nil? :expect X) runner M)
        let Some(expect) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if is_csend(&expect) || expect.receiver().is_some() || method_name(&expect) != "expect" || helpers::rspec::block_of(&expect).is_some() {
            return;
        }
        let [actual] = &send_args(&expect)[..] else { return };
        let [matcher] = &send_args(&call)[..] else { return };
        let strict = cx.cfg.bool("Strict").unwrap_or(false);
        let src = cx.src.text;
        let (s, e) = send_range(&call);
        match cx.cfg.str("EnforcedStyle") {
            Some("inflected") => {
                // predicate_in_actual?: a `matches_<suffix>?` call with a receiver
                let Some(predicate) = send_or_block_send(actual) else { return };
                let pname = method_name(&predicate);
                let is_predicate =
                    pname.strip_prefix("matches_").and_then(|n| n.strip_suffix('?')).is_some_and(|n| SUFFIXES.contains(&n));
                if predicate.receiver().is_none() || !is_predicate {
                    return;
                }
                // boolean_matcher?
                let Some(m) = matcher.as_call_node().filter(|m| !is_csend(m) && m.receiver().is_none() && helpers::rspec::block_of(m).is_none())
                else {
                    return;
                };
                let margs = send_args(&m);
                let mname = method_name(&m);
                let boolthy = margs.is_empty()
                    && matches!(mname, "be_truthy" | "be_falsey" | "be_falsy" | "a_truthy_value" | "a_falsey_value" | "a_falsy_value");
                let bool_arg = |a: &Node| a.as_true_node().is_some() || a.as_false_node().is_some();
                let be_bool = matches!(mname, "be" | "eq" | "eql" | "equal") && matches!(&margs[..], [a] if bool_arg(a));
                if !(boolthy || (!strict && be_bool)) {
                    return;
                }
                let new_matcher = format!("match_{}", &pname["matches_".len()..pname.len() - 1]);
                let msg = format!("Prefer using `{new_matcher}` matcher over `{pname}`.");
                if helpers::rspec::block_of(&predicate).is_some() {
                    return cx.add_with(s, e, msg, false);
                }
                // true?
                let result = match mname {
                    "be" | "eq" | "eql" | "equal" => Some(margs[0].as_true_node().is_some()),
                    "be_truthy" | "a_truthy_value" => Some(true),
                    "be_falsey" | "be_falsy" | "a_falsey_value" | "a_falsy_value" => Some(false),
                    _ => None,
                };
                let positive = if to == "to" { result == Some(true) } else { result.is_none_or(|r| !r) };
                let Some(dot) = predicate.call_operator_loc() else { return };
                let remove = (dot.start_offset(), send_range(&predicate).1);
                let sel = call.message_loc().unwrap();
                let (ms, me) = helpers::rspec::node_range(matcher);
                let replacement = format!("{new_matcher}{}", args_source(src, &predicate));
                cx.add_fix(s, e, msg, |c| {
                    c.remove(remove.0, remove.1);
                    c.replace(sel.start_offset(), sel.end_offset(), if positive { "to" } else { "not_to" });
                    c.replace(ms, me, &replacement);
                });
            }
            Some("explicit") => {
                // predicate_matcher?: (send nil? match_<suffix> ...) or its block form; `expect(nil)` is skipped
                if actual.as_nil_node().is_some() {
                    return;
                }
                let Some(m) = send_or_block_send(matcher).filter(|m| m.receiver().is_none()) else { return };
                let mname = method_name(&m);
                // `cop_config.fetch(...) + BUILT_IN_MATCHERS`: a String raises (a cop error).
                let mut allowed = cx.cfg.array("AllowedExplicitMatchers");
                allowed.extend(BUILT_IN_MATCHERS.iter().map(|s| s.to_string()));
                let inflected = mname.strip_prefix("match_").is_some_and(|n| SUFFIXES.contains(&n));
                if allowed.iter().any(|a| a == mname) || !inflected {
                    return;
                }
                let predicate = format!("{}?", mname.replacen("match_", "matches_", 1));
                let msg = format!("Prefer using `{predicate}` over `{mname}` matcher.");
                if helpers::rspec::block_of(&m).is_some() {
                    return cx.add_with(s, e, msg, false);
                }
                let replacement = match (strict, to == "to") {
                    (true, true) => "be(true)",
                    (true, false) => "be(false)",
                    (false, true) => "be_truthy",
                    (false, false) => "be_falsey",
                };
                let (ms, me) = send_range(&m);
                let (as_, ae) = helpers::rspec::node_range(actual);
                let insert = format!(".{predicate}{}", args_source(src, &m));
                let wrapped = requires_parentheses(cx, actual).then(|| format!("({}){insert}", &src[as_..ae]));
                let sel = call.message_loc().unwrap();
                cx.add_fix(s, e, msg, |c| {
                    c.replace(ms, me, replacement);
                    match &wrapped {
                        Some(w) => c.replace(as_, ae, w),
                        None => c.insert_after(as_, ae, &insert),
                    }
                    c.replace(sel.start_offset(), sel.end_offset(), "to");
                });
            }
            _ => {}
        }
    }
}
