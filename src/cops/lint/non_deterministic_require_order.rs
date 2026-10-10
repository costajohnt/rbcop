use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Lint/NonDeterministicRequireOrder";
pub static COP: NonDeterministicRequireOrder = NonDeterministicRequireOrder;
pub struct NonDeterministicRequireOrder;

const MSG: &str = "Sort files before requiring them.";

/// `(const {nil? cbase} :Dir)`
fn is_dir(node: Option<Node>) -> bool {
    let Some(node) = node else { return false };
    if let Some(c) = node.as_constant_read_node() {
        return c.name().as_slice() == b"Dir";
    }
    node.as_constant_path_node().is_some_and(|p| p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Dir"))
}

fn plain_send(call: &CallNode) -> bool {
    !is_csend(call) && !call.block().is_some_and(|b| b.as_block_node().is_some())
}

/// `(send (const {nil? cbase} :Dir) :glob ...)`
fn dir_glob(call: &CallNode) -> bool {
    !is_csend(call) && method_name(call) == "glob" && is_dir(call.receiver())
}

/// `(send (send (const {nil? cbase} :Dir) {:[] :glob} ...) :each ...)` receiver check.
fn dir_each(call: &CallNode) -> bool {
    !is_csend(call)
        && method_name(call) == "each"
        && call.receiver().and_then(|r| r.as_call_node()).is_some_and(|r| {
            plain_send(&r) && matches!(method_name(&r), "[]" | "glob") && is_dir(r.receiver())
        })
}

/// `(block-pass (send nil? :method (sym {:require :require_relative})))`
fn method_require(node: &Node) -> bool {
    let Some(arg) = node.as_block_argument_node().and_then(|b| b.expression()) else { return false };
    let Some(m) = arg.as_call_node() else { return false };
    let args = send_args(&m);
    m.receiver().is_none()
        && method_name(&m) == "method"
        && args.len() == 1
        && args[0].as_symbol_node().is_some_and(|s| matches!(s.unescaped(), b"require" | b"require_relative"))
}

/// `def_node_search :var_is_required?`: `(send nil? {:require :require_relative} (lvar %1))`.
struct Required<'a> {
    name: &'a [u8],
    found: bool,
}

impl<'pr> Visit<'pr> for Required<'_> {
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        let args = send_args(node);
        if node.receiver().is_none()
            && matches!(method_name(node), "require" | "require_relative")
            && args.len() == 1
            && args[0].as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == self.name)
        {
            self.found = true;
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn required(body: &Node, name: &[u8]) -> bool {
    let mut v = Required { name, found: false };
    v.visit(body);
    v.found
}

impl Cop for NonDeterministicRequireOrder {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (0.0, 2.7)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(blk) = call.block() else { return };
        let (start, end) = send_range(&call);
        if let Some(block) = blk.as_block_node() {
            let Some(body) = block.body() else { return };
            let glob = dir_glob(&call);
            if !glob && !(dir_each(&call) && call_args(&call).is_empty()) {
                return;
            }
            let names: Vec<Vec<u8>> = match block.parameters() {
                // (args (arg $_))
                Some(p) if p.as_block_parameters_node().is_some() => {
                    let bp = p.as_block_parameters_node().unwrap();
                    let Some(params) = bp.parameters() else { return };
                    let all = parameter_nodes(&params);
                    if all.len() != 1 || bp.locals().iter().next().is_some() {
                        return;
                    }
                    let Some(r) = all[0].as_required_parameter_node() else { return };
                    vec![r.name().as_slice().to_vec()]
                }
                Some(p) if p.as_numbered_parameters_node().is_some() => {
                    let max = p.as_numbered_parameters_node().unwrap().maximum();
                    (1..=max).map(|i| format!("_{i}").into_bytes()).collect()
                }
                _ => return,
            };
            let replacement = if glob {
                format!("{}.sort.each", cx.src.slice(start, end))
            } else {
                format!("{}.sort.each", node_text(&call.receiver().unwrap()))
            };
            for name in names {
                if required(&body, &name) {
                    cx.add_fix(start, end, MSG, |c| c.replace(start, end, &replacement));
                }
            }
        } else if method_require(&blk) {
            if dir_glob(&call) {
                // correct_block_pass: drop `, &method(:require)` and append `.sort.each(...)`.
                let args = call_args(&call);
                let Some(prev) = args.last() else { return }; // arguments[-2] is nil: RuboCop crashes
                let cut = prev.location().end_offset();
                let block_end = blk.location().end_offset();
                let text = format!(".sort.each({})", node_text(&blk));
                cx.add_fix(start, end, MSG, |c| {
                    c.remove(cut, block_end);
                    c.insert_after(start, end, &text);
                });
            } else if dir_each(&call) && call_args(&call).is_empty() {
                let sel = call.message_loc().unwrap();
                cx.add_fix(start, end, MSG, |c| c.replace(sel.start_offset(), sel.end_offset(), "sort.each"));
            }
        }
    }
}
