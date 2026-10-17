use crate::prelude::*;

pub const NAME: &str = "RSpec/ExpectChange";
pub static COP: ExpectChange = ExpectChange;
pub struct ExpectChange;

static METHOD_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[A-Za-z_][A-Za-z0-9_]*[?!]?\z").unwrap());

impl Cop for ExpectChange {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let negated = match cx.cfg.get("NegatedMatcher") {
            None | Some(Value::Null | Value::Bool(false)) => None,
            Some(v) => v.as_string(),
        };
        if name != "change" && negated.as_deref() != Some(name) {
            return;
        }
        match cx.cfg.str("EnforcedStyle") {
            Some("block") => {
                // on_send: (send nil? _ $_ ({sym str} $_))
                if is_csend(&call) || call.receiver().is_some() {
                    return;
                }
                let [receiver, message] = &send_args(&call)[..] else { return };
                let value = if helpers::rspec::is_sym(cx, message) || helpers::rspec::is_str(message) {
                    match message.as_symbol_node() {
                        Some(s) => String::from_utf8_lossy(s.unescaped()).into_owned(),
                        None => String::from_utf8_lossy(message.as_string_node().unwrap().unescaped()).into_owned(),
                    }
                } else {
                    return;
                };
                let obj = node_text(receiver);
                let msg = format!("Prefer `{name} {{ {obj}.{value} }}`.");
                let (s, e) = send_range(&call);
                // The attribute becomes a method call: only a plain method name is spliced in.
                if !METHOD_NAME.is_match(&value) {
                    cx.add_withheld_fix(s, e, msg);
                    return;
                }
                let replacement = format!("{name} {{ {obj}.{value} }}");
                cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
            }
            Some("method_call") => {
                // on_block: (block (send nil? _) (args) (send ${(send nil? _) const} $_))
                let Some(block) = helpers::rspec::plain_block_call(node).and_then(|c| helpers::rspec::block_of(&c)) else {
                    return;
                };
                if call.receiver().is_some() || !send_args(&call).is_empty() {
                    return;
                }
                if block.parameters().is_some_and(|p| {
                    p.as_block_parameters_node().is_none_or(|b| b.parameters().is_some() || b.locals().iter().next().is_some())
                }) {
                    return;
                }
                let Some(body) = helpers::rspec::parser_body(block.body()) else { return };
                let Some(send) = helpers::rspec::send_call(&body).filter(|c| !is_csend(c)) else { return };
                if !send_args(&send).is_empty() {
                    return;
                }
                let Some(receiver) = send.receiver() else { return };
                let ok = helpers::rspec::send_call(&receiver)
                    .is_some_and(|r| !is_csend(&r) && r.receiver().is_none() && send_args(&r).is_empty())
                    || matches!(kind::of(&receiver), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE);
                if !ok {
                    return;
                }
                let obj = node_text(&receiver);
                let attr = method_name(&send);
                let msg = format!("Prefer `{name}({obj}, :{attr})`.");
                let replacement = format!("{name}({obj}, :{attr})");
                let (s, e) = helpers::rspec::node_range(node);
                cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
            }
            _ => {}
        }
    }
}
