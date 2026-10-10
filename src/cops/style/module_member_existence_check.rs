use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/ModuleMemberExistenceCheck";
pub static COP: ModuleMemberExistenceCheck = ModuleMemberExistenceCheck;
pub struct ModuleMemberExistenceCheck;

fn replacement_method(name: &str) -> Option<&'static str> {
    Some(match name {
        "class_variables" => "class_variable_defined?",
        "instance_methods" => "method_defined?",
        "private_instance_methods" => "private_method_defined?",
        "protected_instance_methods" => "protected_method_defined?",
        "public_instance_methods" => "public_method_defined?",
        _ => return None,
    })
}

/// `{(call _ %WITH_INHERIT _?) (call _ %WITHOUT_INHERIT)}`
fn member_list(node: &Node) -> bool {
    let Some(c) = node.as_call_node() else { return false };
    let Some(_) = replacement_method(method_name(&c)) else { return false };
    let n = send_args(&c).len();
    if method_name(&c) == "class_variables" { n == 0 } else { n <= 1 }
}

/// `simple_method_argument?`: no splat/restarg, no trailing block pass, first argument not a hash.
fn simple(c: &CallNode) -> bool {
    let args = send_args(c);
    !args.iter().any(|a| a.as_splat_node().is_some())
        && args.last().is_none_or(|a| a.as_block_argument_node().is_none())
        && args.first().is_none_or(|a| a.as_hash_node().is_none() && a.as_keyword_hash_node().is_none())
}

impl Cop for ModuleMemberExistenceCheck {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(repl) = replacement_method(method_name(&call)) else { return };
        let Some(parent) = helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node()) else {
            return;
        };
        // `(call <member list> {:include? :member?} _)`
        if !matches!(method_name(&parent), "include?" | "member?")
            || !parent.receiver().is_some_and(|r| member_list(&r))
            || send_args(&parent).len() != 1
        {
            return;
        }
        if !simple(&call) || !simple(&parent) {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (s, e) = (sel.start_offset(), send_range(&parent).1);
        let parent_arg = node_text(&send_args(&parent)[0]);
        let first = send_args(&call).into_iter().next();
        let replacement = match first {
            Some(a) if method_name(&call) != "class_variables" && a.as_true_node().is_none() => {
                format!("{repl}({parent_arg}, {})", node_text(&a))
            }
            _ => format!("{repl}({parent_arg})"),
        };
        let msg = format!("Use `{replacement}` instead.");
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
    }
}
