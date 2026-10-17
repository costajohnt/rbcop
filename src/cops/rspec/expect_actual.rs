use crate::prelude::*;

pub const NAME: &str = "RSpec/ExpectActual";
pub static COP: ExpectActual = ExpectActual;
pub struct ExpectActual;

/// ExpectActual#literal?: simple literals, and arrays/hashes/pairs/ranges/regexps of literals.
fn literal(cx: &Cx, node: &Node) -> bool {
    match kind::of(node) {
        kind::TRUE_NODE
        | kind::FALSE_NODE
        | kind::NIL_NODE
        | kind::INTEGER_NODE
        | kind::FLOAT_NODE
        | kind::IMAGINARY_NODE
        | kind::RATIONAL_NODE
        | kind::SOURCE_FILE_NODE
        | kind::SOURCE_LINE_NODE => true,
        kind::STRING_NODE => helpers::rspec::is_str(node),
        kind::SYMBOL_NODE => !helpers::rspec::is_dsym(cx, node),
        kind::ARRAY_NODE => node.as_array_node().unwrap().elements().iter().all(|e| literal(cx, &e)),
        kind::HASH_NODE => node.as_hash_node().unwrap().elements().iter().all(|e| literal(cx, &e)),
        kind::KEYWORD_HASH_NODE => node.as_keyword_hash_node().unwrap().elements().iter().all(|e| literal(cx, &e)),
        kind::ASSOC_NODE => {
            let a = node.as_assoc_node().unwrap();
            literal(cx, &a.key()) && (a.value().as_implicit_node().is_none() && literal(cx, &a.value()))
        }
        kind::RANGE_NODE => {
            let r = node.as_range_node().unwrap();
            r.left().is_none_or(|n| literal(cx, &n)) && r.right().is_none_or(|n| literal(cx, &n))
        }
        // (regexp (str ...)... (regopt)): no interpolation
        kind::REGULAR_EXPRESSION_NODE => true,
        kind::INTERPOLATED_REGULAR_EXPRESSION_NODE => {
            node.as_interpolated_regular_expression_node().unwrap().parts().iter().all(|p| p.as_string_node().is_some())
        }
        _ => false,
    }
}

impl Cop for ExpectActual {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let send = node.as_call_node().unwrap();
        if is_csend(&send) || !helpers::rspec::is_runner(method_name(&send)) {
            return;
        }
        // (send nil? :expect $#literal?)
        let Some(expect) = send.receiver().and_then(|r| helpers::rspec::nil_send(&r, "expect")) else { return };
        let [actual] = &send_args(&expect)[..] else { return };
        if !literal(cx, actual) {
            return;
        }
        let [arg] = &send_args(&send)[..] else { return };
        let Some(matcher_send) = helpers::rspec::send_call(arg).filter(|c| !is_csend(c)) else { return };
        // {(send (send nil? $:be) :== $_) (send nil? $_ $_ ...)} or (send nil? $_)
        let margs = send_args(&matcher_send);
        let (matcher, expected): (String, Option<&Node>) = if method_name(&matcher_send) == "=="
            && margs.len() == 1
            && matcher_send.receiver().and_then(|r| helpers::rspec::bare_send(&r, "be")).is_some()
        {
            ("be".to_string(), Some(&margs[0]))
        } else if matcher_send.receiver().is_none() {
            (method_name(&matcher_send).to_string(), margs.first())
        } else {
            return;
        };
        if matches!(matcher.as_str(), "route_to" | "be_routable") {
            return;
        }
        let (as_, ae) = helpers::rspec::node_range(actual);
        let msg = if expected.is_none() {
            "Test a non-literal value with `expect(...)`."
        } else {
            "Provide the actual value you are testing to `expect(...)`."
        };
        let correctable = matches!(matcher.as_str(), "eq" | "eql" | "equal" | "be")
            && expected.is_some_and(|e| !literal(cx, e));
        if !correctable {
            cx.add_fix(as_, ae, msg, |_| {});
            return;
        }
        let expected = expected.unwrap();
        let src = cx.src;
        let actual_src = src.slice(as_, ae).to_string();
        let (es, ee) = helpers::rspec::node_range(expected);
        let expected_src = src.slice(es, ee).to_string();
        let (ms, me) = helpers::rspec::node_range(&matcher_send.as_node());
        cx.add_fix(as_, ae, msg, |c| {
            c.replace(as_, ae, &expected_src);
            if matcher == "be" {
                c.replace(es, ee, &actual_src);
            } else {
                c.replace(ms, me, &format!("{matcher}({actual_src})"));
            }
        });
    }
}
