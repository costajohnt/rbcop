//! Node patterns shared by Style/FileRead, FileWrite and FileTouch.

use crate::prelude::*;

/// Parser `(str $%SET)`: the value when it is one of `set`.
pub fn mode_str(node: &Node, set: &[&'static [u8]]) -> Option<&'static [u8]> {
    let s = node.as_string_node()?;
    if is_parser_dstr(node) {
        return None;
    }
    let v = s.unescaped();
    set.iter().find(|m| **m == v).copied()
}

/// `(block-pass (sym :name))`
pub fn is_sym_block_pass(node: &Node, name: &str) -> bool {
    node.as_block_argument_node()
        .and_then(|b| b.expression())
        .and_then(|e| e.as_symbol_node())
        .is_some_and(|s| s.unescaped() == name.as_bytes())
}

/// `(block _ (args (arg _name)) (send (lvar _name) <method> ...))` on a call owning a block:
/// the body send.
pub fn single_arg_block_send<'pr>(call: &ruby_prism::CallNode<'pr>, method: &str) -> Option<ruby_prism::CallNode<'pr>> {
    let block = call.block()?.as_block_node()?;
    let params = block.parameters()?.as_block_parameters_node()?;
    if params.locals().iter().next().is_some() {
        return None;
    }
    let p = params.parameters()?;
    if p.optionals().iter().next().is_some()
        || parser_rest(p.rest()).is_some()
        || p.posts().iter().next().is_some()
        || p.keywords().iter().next().is_some()
        || p.keyword_rest().is_some()
        || p.block().is_some()
    {
        return None;
    }
    let reqs: Vec<Node> = p.requireds().iter().collect();
    let [arg] = reqs.as_slice() else { return None };
    let arg = arg.as_required_parameter_node()?;
    let body = block.body()?.as_statements_node()?;
    let stmts: Vec<Node> = body.body().iter().collect();
    let [stmt] = stmts.as_slice() else { return None };
    let send = stmt.as_call_node()?;
    if is_csend(&send) || method_name(&send) != method || send.block().is_some_and(|b| b.as_block_node().is_some()) {
        return None;
    }
    let recv = send.receiver()?;
    let lvar = recv.as_local_variable_read_node()?;
    (lvar.name().as_slice() == arg.name().as_slice()).then_some(send)
}
