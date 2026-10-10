use crate::prelude::*;
use ruby_prism::StatementsNode;

pub const NAME: &str = "Lint/UnreachableCode";
pub static COP: UnreachableCode = UnreachableCode;
pub struct UnreachableCode;

/// `@redefined`: flow methods redefined by a `def` seen so far in the file.
#[derive(Default)]
struct Redefined(Vec<String>);

struct Check<'a> {
    redefined: &'a mut Vec<String>,
    in_instance_eval: bool,
}

const FLOW_METHODS: [&str; 6] = ["raise", "fail", "throw", "exit", "exit!", "abort"];

impl Check<'_> {
    fn flow_expression(&mut self, node: &Node) -> bool {
        match kind::of(node) {
            kind::RETURN_NODE | kind::NEXT_NODE | kind::BREAK_NODE | kind::RETRY_NODE | kind::REDO_NODE => true,
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                if is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
                    return false;
                }
                let name = method_name(&call);
                if !FLOW_METHODS.contains(&name) {
                    return false;
                }
                match call.receiver() {
                    None => !self.in_instance_eval && !self.redefined.iter().any(|r| r == name),
                    Some(r) => {
                        if let Some(c) = r.as_constant_read_node() {
                            c.name().as_slice() == b"Kernel"
                        } else if let Some(p) = r.as_constant_path_node() {
                            p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Kernel")
                        } else {
                            false
                        }
                    }
                }
            }
            kind::PARENTHESES_NODE => {
                let body = node.as_parentheses_node().unwrap().body();
                match body {
                    Some(b) => match b.as_statements_node() {
                        Some(s) => s.body().iter().any(|e| self.flow_expression(&e)),
                        None => self.flow_expression(&b),
                    },
                    None => false,
                }
            }
            kind::BEGIN_NODE => {
                let b = node.as_begin_node().unwrap();
                if b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                    return false;
                }
                b.statements().is_some_and(|s| s.body().iter().any(|e| self.flow_expression(&e)))
            }
            kind::IF_NODE => {
                let f = node.as_if_node().unwrap();
                let Some(if_branch) = f.statements() else { return false };
                let Some(sub) = f.subsequent() else { return false };
                let else_body = match sub.as_else_node() {
                    Some(e) => match e.statements() {
                        Some(s) => Some(s.as_node()),
                        None => return false,
                    },
                    None => Some(sub),
                };
                self.body_flow(&if_branch) && else_body.is_some_and(|e| self.node_flow(&e))
            }
            kind::UNLESS_NODE => {
                let f = node.as_unless_node().unwrap();
                let Some(els) = f.else_clause().and_then(|e| e.statements()) else { return false };
                let Some(body) = f.statements() else { return false };
                self.body_flow(&els) && self.body_flow(&body)
            }
            kind::CASE_NODE => {
                let c = node.as_case_node().unwrap();
                let Some(els) = c.else_clause().and_then(|e| e.statements()) else { return false };
                if !self.body_flow(&els) {
                    return false;
                }
                c.conditions().iter().all(|w| {
                    w.as_when_node().unwrap().statements().is_some_and(|s| self.body_flow(&s))
                })
            }
            kind::CASE_MATCH_NODE => {
                let c = node.as_case_match_node().unwrap();
                let Some(els) = c.else_clause().and_then(|e| e.statements()) else { return false };
                if !self.body_flow(&els) {
                    return false;
                }
                c.conditions().iter().all(|w| w.as_in_node().unwrap().statements().is_some_and(|s| self.body_flow(&s)))
            }
            kind::DEF_NODE => {
                let name = String::from_utf8_lossy(node.as_def_node().unwrap().name().as_slice()).into_owned();
                if FLOW_METHODS.contains(&name.as_str()) {
                    self.redefined.push(name);
                }
                false
            }
            _ => false,
        }
    }

    /// A statements body as parser sees it: one node, or a `begin` of several.
    fn body_flow(&mut self, s: &StatementsNode) -> bool {
        let list: Vec<Node> = s.body().iter().collect();
        if list.len() == 1 {
            self.flow_expression(&list[0])
        } else {
            list.iter().any(|e| self.flow_expression(e))
        }
    }

    fn node_flow(&mut self, n: &Node) -> bool {
        match n.as_statements_node() {
            Some(s) => self.body_flow(&s),
            None => self.flow_expression(n),
        }
    }
}

fn instance_eval_block(n: &Node) -> bool {
    n.as_call_node()
        .is_some_and(|c| method_name(&c) == "instance_eval" && c.block().is_some_and(|b| b.as_block_node().is_some()))
}

impl Cop for UnreachableCode {
    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let exprs: Vec<Node> = node.as_statements_node().unwrap().body().iter().collect();
        if exprs.len() < 2 {
            return;
        }
        let in_instance_eval = cx.ancestors.iter().any(instance_eval_block);
        let mut redefined = std::mem::take(&mut cx.state::<Redefined>().0);
        let mut check = Check { redefined: &mut redefined, in_instance_eval };
        let mut reached = false;
        let mut hits = Vec::new();
        for (i, e) in exprs.iter().enumerate() {
            if reached {
                let l = e.location();
                hits.push((l.start_offset(), l.end_offset()));
            } else if i < exprs.len() - 1 && check.flow_expression(e) {
                reached = true;
            }
        }
        cx.state::<Redefined>().0 = redefined;
        for (s, e) in hits {
            cx.add(s, e, "Unreachable code detected.");
        }
    }
}
