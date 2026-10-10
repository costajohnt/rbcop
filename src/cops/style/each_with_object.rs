use crate::helpers::literal_node::basic_literal;
use crate::helpers::ruby_string::strip;
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/EachWithObject";
pub static COP: EachWithObject = EachWithObject;
pub struct EachWithObject;

/// `return_value(body)`: the body (or the last child of a `begin` body) when it is an lvar.
fn return_value<'pr>(body: Option<Node<'pr>>) -> Option<ruby_prism::LocalVariableReadNode<'pr>> {
    let body = body?;
    let stmts = body.as_statements_node()?;
    let list: Vec<Node> = stmts.body().iter().collect();
    let node = if list.len() == 1 {
        let only = list.into_iter().next()?;
        match only.as_parentheses_node() {
            // `(x)` is a parser `begin`: take its last child
            Some(p) => match p.body()?.as_statements_node() {
                Some(s) => s.body().iter().last()?,
                None => p.body()?,
            },
            None => only,
        }
    } else {
        list.into_iter().last()?
    };
    node.as_local_variable_read_node()
}

/// Any `lvasgn` of `name` among the body's descendants.
struct Assigns<'a> {
    name: &'a [u8],
    hit: bool,
}

impl<'pr> Visit<'pr> for Assigns<'_> {
    fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.hit |= n.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_write_node(self, n);
    }
    fn visit_local_variable_target_node(&mut self, n: &ruby_prism::LocalVariableTargetNode<'pr>) {
        self.hit |= n.name().as_slice() == self.name;
    }
    fn visit_local_variable_operator_write_node(&mut self, n: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.hit |= n.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_operator_write_node(self, n);
    }
    fn visit_local_variable_or_write_node(&mut self, n: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.hit |= n.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_or_write_node(self, n);
    }
    fn visit_local_variable_and_write_node(&mut self, n: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.hit |= n.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_and_write_node(self, n);
    }
}

/// `_1`/`_2` lvars among the body's descendants.
struct NumberedVars(Vec<(usize, usize, bool)>);

impl<'pr> Visit<'pr> for NumberedVars {
    fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
        let loc = n.location();
        match loc.as_slice() {
            b"_1" => self.0.push((loc.start_offset(), loc.end_offset(), true)),
            b"_2" => self.0.push((loc.start_offset(), loc.end_offset(), false)),
            _ => {}
        }
    }
}

/// Name of a parser arg's first child (`arg`, `optarg`, `restarg`, ... name).
fn param_name<'pr>(node: &Node<'pr>) -> Option<&'pr [u8]> {
    if let Some(p) = node.as_required_parameter_node() {
        return Some(p.name().as_slice());
    }
    if let Some(p) = node.as_optional_parameter_node() {
        return Some(p.name().as_slice());
    }
    if let Some(p) = node.as_rest_parameter_node() {
        return p.name().map(|n| n.as_slice());
    }
    if let Some(p) = node.as_required_keyword_parameter_node() {
        return Some(p.name().as_slice());
    }
    if let Some(p) = node.as_optional_keyword_parameter_node() {
        return Some(p.name().as_slice());
    }
    if let Some(p) = node.as_keyword_rest_parameter_node() {
        return p.name().map(|n| n.as_slice());
    }
    if let Some(p) = node.as_block_parameter_node() {
        return p.name().map(|n| n.as_slice());
    }
    None
}

impl Cop for EachWithObject {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "inject" | "reduce") {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let args = call_args(&call);
        let [method_arg] = args.as_slice() else { return };
        if basic_literal(method_arg) {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let msg = format!("Use `each_with_object` instead of `{name}`.");
        let params = block.parameters();
        if let Some(n) = params.as_ref().and_then(|p| p.as_numbered_parameters_node()) {
            if n.maximum() != 2 || return_value(block.body()).is_none_or(|r| r.location().as_slice() != b"_1") {
                return;
            }
            let mut vars = NumberedVars(Vec::new());
            if let Some(body) = block.body() {
                let stmts: Vec<Node> = body.as_statements_node().map(|s| s.body().iter().collect()).unwrap_or_default();
                if stmts.len() == 1 {
                    // the single statement is parser's body: only its descendants
                    vars.visit(&stmts[0]);
                    let own = stmts[0].location().start_offset();
                    vars.0.retain(|v| !(v.0 == own && stmts[0].as_local_variable_read_node().is_some()));
                } else {
                    vars.visit(&body);
                }
            }
            return cx.add_fix(ss, se, msg, |c| {
                c.replace(ss, se, "each_with_object");
                for (s, e, one) in vars.0 {
                    c.replace(s, e, if one { "_2" } else { "_1" });
                }
            });
        }
        let Some(bp) = params.as_ref().and_then(|p| p.as_block_parameters_node()) else { return };
        let mut list: Vec<Node> = bp.parameters().map(|p| parameter_nodes(&p)).unwrap_or_default();
        list.extend(bp.locals().iter());
        let [first, second] = list.as_slice() else { return };
        let Some(ret) = return_value(block.body()) else { return };
        // first_argument_returned?
        if param_name(first) != Some(ret.name().as_slice()) {
            return;
        }
        let mut assigns = Assigns { name: ret.name().as_slice(), hit: false };
        if let Some(body) = block.body() {
            assigns.visit(&body);
        }
        if assigns.hit {
            return;
        }
        let a = (first.location().start_offset(), first.location().end_offset());
        let b = (second.location().start_offset(), second.location().end_offset());
        let (rs, re) = (ret.location().start_offset(), ret.location().end_offset());
        let text = cx.src.text;
        let whole = range_by_whole_lines(text, rs, re, true);
        let remove = if strip(&text[whole.0..whole.1]) == &text[rs..re] { whole } else { (rs, re) };
        cx.add_fix(ss, se, msg, |c| {
            c.replace(ss, se, "each_with_object");
            c.swap(a, b);
            c.remove(remove.0, remove.1);
        });
    }
}
