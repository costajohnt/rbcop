use crate::helpers::parser_parent::is_top_const;
use crate::helpers::sorbet_signature::{as_send, block_body, signature};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Sorbet/CapitalizedTypeParameters";
pub static COP: CapitalizedTypeParameters = CapitalizedTypeParameters;
pub struct CapitalizedTypeParameters;

/// `/^[A-Z]/` (any line of the value).
static CAPITALIZED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[A-Z]").unwrap());

/// String#capitalize.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |f| f.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect())
}

fn check_type_parameters_case(cx: &mut Cx, call: &CallNode) {
    for arg in send_args(call) {
        let Some(sym) = arg.as_symbol_node() else { continue };
        let Ok(value) = std::str::from_utf8(sym.unescaped()) else { continue };
        if CAPITALIZED.is_match(value) {
            continue;
        }
        let fixed = helpers::ruby_string::symbol_inspect(capitalize(value).as_bytes());
        let (s, e) = helpers::node_types::range(&arg);
        cx.add_fix(s, e, "Type parameters must be capitalized.", |c| c.replace(s, e, &fixed));
    }
}

impl Cop for CapitalizedTypeParameters {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if signature(node).is_some() {
            // on_signature: the `type_parameters(...)` calls along the body's send chain.
            let mut cur = block_body(&call);
            while let Some(send) = cur.as_ref().and_then(|n| as_send(n)) {
                if send.receiver().is_none() && send.name().as_slice() == b"type_parameters" {
                    check_type_parameters_case(cx, &send);
                }
                cur = send.receiver();
            }
        }
        // on_send: `T.type_parameter(...)`.
        if !is_csend(&call)
            && call.name().as_slice() == b"type_parameter"
            && call.receiver().is_some_and(|r| is_top_const(&r, "T"))
        {
            check_type_parameters_case(cx, &call);
        }
    }
}
