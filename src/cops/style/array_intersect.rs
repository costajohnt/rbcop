use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/ArrayIntersect";
pub static COP: ArrayIntersect = ArrayIntersect;
pub struct ArrayIntersect;

const STRAIGHT: &[&str] = &["present?", "any?", ">", "positive?", "!="];
const NEGATED: &[&str] = &["blank?", "empty?", "none?", "==", "zero?"];

/// The single argument of a call without block pass or block.
fn single_arg<'pr>(call: &CallNode<'pr>) -> Option<Node<'pr>> {
    if call.block().is_some() {
        return None;
    }
    let mut args = call_args(call);
    if args.len() != 1 {
        return None;
    }
    args.pop()
}

/// `{(begin (send $_ :& $_)) (call $!nil? :intersection $_)}`: (receiver, argument).
fn intersection<'pr>(node: &Node<'pr>) -> Option<(Node<'pr>, Node<'pr>)> {
    if let Some(p) = node.as_parentheses_node() {
        let stmts = p.body()?.as_statements_node()?;
        let mut list: Vec<Node> = stmts.body().iter().collect();
        if list.len() != 1 {
            return None;
        }
        let call = list.pop()?.as_call_node()?;
        if is_csend(&call) || method_name(&call) != "&" {
            return None;
        }
        let arg = single_arg(&call)?;
        return Some((call.receiver()?, arg));
    }
    let call = node.as_call_node()?;
    if method_name(&call) != "intersection" {
        return None;
    }
    let arg = single_arg(&call)?;
    Some((call.receiver()?, arg))
}

fn dot_of(call: &CallNode) -> Option<String> {
    call.call_operator_loc().map(|d| text(&d).to_string())
}

fn register(cx: &mut Cx, s: usize, e: usize, replacement: String) {
    let msg = format!("Use `{replacement}` instead of `{}`.", cx.src.slice(s, e));
    cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
}

impl Cop for ArrayIntersect {
    fn correctable(&self) -> bool {
        true
    }
    fn ruby_range(&self) -> (f64, f64) {
        (3.1, f64::MAX)
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            on_block(cx, node, &call);
            return;
        }
        let name = method_name(&call);
        if !STRAIGHT.contains(&name) && !NEGATED.contains(&name) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        if call.block().is_some() {
            return;
        }
        let args = call_args(&call);
        let mut preds = vec!["any?", "empty?", "none?"];
        if cx.config.all_cops().truthy("ActiveSupportExtensionsEnabled") {
            preds.extend(["present?", "blank?"]);
        }
        // (dot of the call whose `loc.dot` RuboCop reads, receiver, argument)
        let mut found: Option<(Option<String>, Node, Node)> = None;
        if args.is_empty()
            && preds.contains(&name)
            && let Some((r, a)) = intersection(&recv)
        {
            found = Some((dot_of(&call), r, a));
        }
        if found.is_none()
            && let Some(size) = recv.as_call_node()
            && matches!(method_name(&size), "count" | "length" | "size")
            && size.arguments().is_none()
            && size.block().is_none()
            && let Some(inner) = size.receiver()
            && let Some((r, a)) = intersection(&inner)
        {
            let ok = match (name, args.as_slice()) {
                (">" | "!=" | "==", [arg]) => int_literal_value(arg) == Some(0),
                ("positive?" | "zero?", []) => true,
                _ => false,
            };
            if ok {
                found = Some((dot_of(&size), r, a));
            }
        }
        let Some((Some(dot), receiver, argument)) = found else { return };
        let bang = if STRAIGHT.contains(&name) { "" } else { "!" };
        if bang == "!" && dot == "&." {
            return;
        }
        let replacement = format!("{bang}{}{dot}intersect?({})", node_text(&receiver), node_text(&argument));
        let (s, e) = send_range(&call);
        register(cx, s, e, replacement);
    }
}

fn on_block(cx: &mut Cx, node: &Node, call: &CallNode) {
    let block = call.block().unwrap().as_block_node().unwrap();
    let name = method_name(call);
    if (name != "any?" && name != "none?") || call.arguments().is_some() {
        return;
    }
    let Some(receiver) = call.receiver() else { return };
    let Some(body) = block.body() else { return };
    let Some(stmts) = body.as_statements_node() else { return };
    let mut list: Vec<Node> = stmts.body().iter().collect();
    if list.len() != 1 {
        return;
    }
    let Some(inner) = list.pop().unwrap().as_call_node() else { return };
    if is_csend(&inner) {
        return;
    }
    let block_method = method_name(&inner);
    if block_method != "member?" && block_method != "include?" {
        return;
    }
    let Some(argument) = inner.receiver() else { return };
    let Some(lvar) = single_arg(&inner) else { return };
    let matches_param = match block.parameters() {
        Some(p) if p.as_block_parameters_node().is_some() => {
            let bp = p.as_block_parameters_node().unwrap();
            let params = bp.parameters();
            let list = params.as_ref().map(parameter_nodes).unwrap_or_default();
            bp.locals().iter().next().is_none()
                && list.len() == 1
                && list[0].as_required_parameter_node().is_some_and(|req| {
                    lvar.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == req.name().as_slice())
                })
        }
        Some(p) if p.as_numbered_parameters_node().is_some() => {
            p.as_numbered_parameters_node().unwrap().maximum() == 1
                && lvar.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == b"_1")
        }
        Some(p) if p.as_it_parameters_node().is_some() => lvar.as_it_local_variable_read_node().is_some(),
        _ => false,
    };
    if !matches_param {
        return;
    }
    if name == "none?" && is_csend(call) {
        return;
    }
    if block_method == "include?" && argument.as_array_node().is_none() {
        return;
    }
    let Some(dot) = call.call_operator_loc() else { return };
    let bang = if name == "any?" { "" } else { "!" };
    let replacement = format!("{bang}{}{}intersect?({})", node_text(&receiver), text(&dot), node_text(&argument));
    let loc = node.location();
    register(cx, loc.start_offset(), loc.end_offset(), replacement);
}
