use crate::prelude::*;

pub const NAME: &str = "Lint/NestedMethodDefinition";
pub static COP: NestedMethodDefinition = NestedMethodDefinition;
pub struct NestedMethodDefinition;

impl Cop for NestedMethodDefinition {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        // `def x.foo` on a variable, constant or call receiver is allowed.
        if let Some(subject) = def.receiver().and_then(strip_parens) {
            let allowed = matches!(
                subject,
                Node::LocalVariableReadNode { .. }
                    | Node::InstanceVariableReadNode { .. }
                    | Node::GlobalVariableReadNode { .. }
                    | Node::ClassVariableReadNode { .. }
                    | Node::ConstantReadNode { .. }
                    | Node::ConstantPathNode { .. }
                    | Node::CallNode { .. }
            );
            if allowed {
                return;
            }
        }
        if !cx.ancestors.iter().any(|a| a.as_def_node().is_some()) {
            return;
        }
        let within_scoping_def = cx.ancestors.iter().enumerate().any(|(i, a)| match a {
            Node::SingletonClassNode { .. } => true,
            Node::BlockNode { .. } | Node::LambdaNode { .. } => scoping_block(cx, a, i),
            _ => false,
        });
        if !within_scoping_def {
            let loc = node.location();
            cx.add(loc.start_offset(), loc.end_offset(), "Method definitions must not be nested. Use `lambda` instead.");
        }
    }
}

fn scoping_block(cx: &Cx, block: &Node, i: usize) -> bool {
    let owner = if block.as_block_node().is_some() { i.checked_sub(1).map(|p| &cx.ancestors[p]) } else { None };
    let call = owner.and_then(|o| o.as_call_node());
    if let Some(call) = &call {
        let name = method_name(call);
        let send = !is_csend(call);
        let eval_or_exec = matches!(
            name,
            "instance_eval" | "class_eval" | "module_eval" | "instance_exec" | "class_exec" | "module_exec"
        );
        if (send && eval_or_exec) || helpers::macro_scope::class_constructor(call) {
            return true;
        }
    }
    // allowed_method_name?: the block's method name (`lambda` for `->`, `super` for super blocks).
    let name = match (block, owner, &call) {
        (_, _, Some(c)) => method_name(c),
        (Node::LambdaNode { .. }, _, _) => "lambda",
        _ => "super",
    };
    helpers::allowed::allowed_method(cx.cfg, name) || helpers::allowed::matches_allowed_pattern(cx.cfg, name)
}
