use crate::prelude::*;

pub const NAME: &str = "Style/MethodCallWithoutArgsParentheses";
pub static COP: MethodCallWithoutArgsParentheses = MethodCallWithoutArgsParentheses;
pub struct MethodCallWithoutArgsParentheses;

impl Cop for MethodCallWithoutArgsParentheses {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let (Some(open), Some(close)) = (call.opening_loc(), call.closing_loc()) else { return };
        if open.as_slice() != b"(" || !send_args(&call).is_empty() {
            return;
        }
        let name = method_name(&call);
        // camel_case_method? / implicit_call? / prefix_not?
        if name.starts_with(|c: char| c.is_ascii_uppercase())
            || (name == "call" && call.message_loc().is_none())
            || (name == "!" && call.message_loc().is_some_and(|s| s.as_slice() == b"not"))
        {
            return;
        }
        if helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(|p| p.as_optional_parameter_node().is_some()) {
            return;
        }
        if helpers::allowed::allowed_method(cx.cfg, name) || helpers::allowed::matches_allowed_pattern(cx.cfg, name) {
            return;
        }
        if call.receiver().is_none() && same_name_assignment(cx.ancestors, name) {
            return;
        }
        if name == "it" && parenthesized_it_in_block(cx.ancestors, &call) {
            return;
        }
        let (start, end) = (open.start_offset(), close.end_offset());
        let (first, last) = (cx.src.line_of(start), cx.src.line_of(end));
        if cx.src.comments.iter().any(|c| (first..=last).contains(&cx.src.line_of(c.start))) {
            return;
        }
        cx.add_fix(start, end, "Do not use parentheses for method calls with no arguments.", |c| c.remove(start, end));
    }
}

/// `same_name_assignment?`: an enclosing (non-call) assignment to a variable of that name.
fn same_name_assignment(ancestors: &[Node], name: &str) -> bool {
    let name = name.as_bytes();
    ancestors.iter().any(|a| {
        if let Some(m) = a.as_multi_write_node() {
            let mut targets = m.lefts().iter().chain(m.rest()).chain(m.rights().iter()).collect::<Vec<_>>();
            while let Some(t) = targets.pop() {
                if let Some(mt) = t.as_multi_target_node() {
                    targets.extend(mt.lefts().iter().chain(mt.rest()).chain(mt.rights().iter()));
                } else if let Some(s) = t.as_splat_node() {
                    targets.extend(s.expression());
                } else if target_name(&t).is_some_and(|n| n == name) {
                    return true;
                }
            }
            return false;
        }
        assignment_name(a).is_some_and(|n| n == name)
    })
}

/// `n.name` of a masgn target that is not a send.
fn target_name<'pr>(t: &Node<'pr>) -> Option<&'pr [u8]> {
    Some(match kind::of(t) {
        kind::LOCAL_VARIABLE_TARGET_NODE => t.as_local_variable_target_node()?.name().as_slice(),
        kind::INSTANCE_VARIABLE_TARGET_NODE => t.as_instance_variable_target_node()?.name().as_slice(),
        kind::CLASS_VARIABLE_TARGET_NODE => t.as_class_variable_target_node()?.name().as_slice(),
        kind::GLOBAL_VARIABLE_TARGET_NODE => t.as_global_variable_target_node()?.name().as_slice(),
        kind::CONSTANT_TARGET_NODE => t.as_constant_target_node()?.name().as_slice(),
        kind::CONSTANT_PATH_TARGET_NODE => t.as_constant_path_target_node()?.name()?.as_slice(),
        _ => return None,
    })
}

/// `asgn_node.loc.name.source` for a variable/constant assignment (shorthand on a call excluded).
fn assignment_name<'pr>(a: &Node<'pr>) -> Option<&'pr [u8]> {
    let call_lhs = [
        kind::MULTI_WRITE_NODE,
        kind::CALL_OPERATOR_WRITE_NODE,
        kind::CALL_OR_WRITE_NODE,
        kind::CALL_AND_WRITE_NODE,
        kind::INDEX_OPERATOR_WRITE_NODE,
        kind::INDEX_OR_WRITE_NODE,
        kind::INDEX_AND_WRITE_NODE,
    ];
    if call_lhs.contains(&kind::of(a)) {
        return None;
    }
    let (op, _) = assignment_parts(a)?;
    let lhs = &a.location().as_slice()[..op.start_offset() - a.location().start_offset()];
    let lhs = lhs.trim_ascii_end();
    // Constant paths (`A::B = 1`): loc.name is the last segment.
    Some(lhs.rsplit(|&b| b == b':').next().unwrap_or(lhs))
}

/// `parenthesized_it_method_in_block?`
fn parenthesized_it_in_block(ancestors: &[Node], call: &ruby_prism::CallNode) -> bool {
    let Some(block) = ancestors.iter().rev().find(|a| matches!(kind::of(a), kind::BLOCK_NODE | kind::LAMBDA_NODE)) else {
        return false;
    };
    let params = match (block.as_block_node(), block.as_lambda_node()) {
        (Some(b), _) => b.parameters(),
        (_, Some(l)) => l.parameters(),
        _ => None,
    };
    // A plain block with `||` or parameters keeps the offense.
    if params.is_some_and(|p| p.as_block_parameters_node().is_some()) {
        return false;
    }
    call.receiver().is_none() && call.block().is_none_or(|b| b.as_block_node().is_none())
}
