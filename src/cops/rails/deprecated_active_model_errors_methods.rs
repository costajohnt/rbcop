use crate::prelude::*;

pub const NAME: &str = "Rails/DeprecatedActiveModelErrorsMethods";
pub static COP: DeprecatedActiveModelErrorsMethods = DeprecatedActiveModelErrorsMethods;
pub struct DeprecatedActiveModelErrorsMethods;

const MANIPULATIVE: &[&str] = &[
    "<<", "append", "clear", "collect!", "compact!", "concat", "delete", "delete_at", "delete_if", "drop",
    "drop_while", "fill", "filter!", "keep_if", "flatten!", "insert", "map!", "pop", "prepend", "push", "reject!",
    "replace", "reverse!", "rotate!", "select!", "shift", "shuffle!", "slice!", "sort!", "sort_by!", "uniq!", "unshift",
];

/// (send #receiver_matcher :errors): the receiver is a send, ivar or lvar (or absent in a model file).
fn errors(n: &Node, model: bool) -> bool {
    let Some(c) = helpers::rails::send_node(n) else { return false };
    if method_name(&c) != "errors" || !send_args(&c).is_empty() {
        return false;
    }
    match c.receiver() {
        None => model,
        Some(r) => {
            helpers::rails::send_node(&r).is_some()
                || r.as_instance_variable_read_node().is_some()
                || helpers::node_types::lvar_name(&r).is_some()
        }
    }
}

/// (send (send R :errors) {:messages :details})
fn messages_or_details(n: &Node, model: bool) -> bool {
    helpers::rails::send_node(n).is_some_and(|c| {
        matches!(method_name(&c), "messages" | "details") && send_args(&c).is_empty() && c.receiver().is_some_and(|r| errors(&r, model))
    })
}

/// (send X :[] ...)
fn index_of<'pr>(n: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    helpers::rails::send_node(n).filter(|c| method_name(c) == "[]")
}

impl Cop for DeprecatedActiveModelErrorsMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let model = cx.src.path.contains("/models/");
        let name = method_name(&call);
        let Some(recv) = call.receiver() else { return };
        let idx_on = |pred: &dyn Fn(&Node) -> bool| index_of(&recv).is_some_and(|i| i.receiver().is_some_and(|r| pred(&r)));
        let matched = (MANIPULATIVE.contains(&name) && idx_on(&|r| errors(r, model)))
            || (name == "[]=" && errors(&recv, model))
            || (matches!(name, "keys" | "values" | "to_h" | "to_xml") && send_args(&call).is_empty() && errors(&recv, model))
            || (MANIPULATIVE.contains(&name) && idx_on(&|r| messages_or_details(r, model)))
            || (name == "[]=" && messages_or_details(&recv, model));
        if !matched {
            return;
        }
        if cx.config.target_rails_version() <= 6.0 && matches!(name, "keys" | "values" | "to_h" | "to_xml") {
            return;
        }
        let (s, e) = send_range(&call);
        // skip_autocorrect?
        let mut skip = !matches!(name, "<<" | "clear" | "keys");
        if !skip
            && let Some(rr) = recv.as_call_node().and_then(|c| c.receiver())
        {
            skip = helpers::rails::send_node(&rr).is_some_and(|c| method_name(&c) == "details") && name == "<<";
        }
        let fix = if skip {
            None
        } else {
            // offense_range: from the end of the `errors` send to the end of the node.
            let mut r: Node = recv.as_call_node().unwrap().as_node();
            while let Some(c) = helpers::rails::send_node(&r) {
                if method_name(&c) == "errors" {
                    break;
                }
                match c.receiver() {
                    Some(x) => r = x,
                    None => break,
                }
            }
            let from = r.location().end_offset();
            let replacement = if name == "keys" {
                ".attribute_names".to_string()
            } else {
                let key = send_args(&recv.as_call_node().unwrap()).first().map(|k| node_text(k).to_string());
                let Some(key) = key else { return };
                if name == "<<" {
                    let Some(v) = send_args(&call).first().map(|v| node_text(v).to_string()) else { return };
                    format!(".add({key}, {v})")
                } else {
                    format!(".delete({key})")
                }
            };
            Some((from, replacement))
        };
        cx.add_fix(s, e, "Avoid manipulating ActiveModel errors as hash directly.", |c| {
            if let Some((from, r)) = &fix {
                c.replace(*from, e, r);
            }
        });
    }
}
