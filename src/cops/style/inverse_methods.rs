use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Style/InverseMethods";
pub static COP: InverseMethods = InverseMethods;
pub struct InverseMethods;

static CAMEL_CASE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Z]+[a-z]+").unwrap());

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// `cop_config[key].merge(cop_config[key].invert)` lookup. Keys are YAML symbols (`:any?`).
fn inverse_of(cx: &Cx, key: &str, method: &str) -> Option<String> {
    let Some(Value::Map(m)) = cx.cfg.get(key) else { return None };
    let sym = |v: &Value| v.as_string().map(|s| s.strip_prefix(':').unwrap_or(&s).to_string());
    let pairs: Vec<(String, String)> =
        m.0.iter().filter_map(|(k, v)| Some((k.strip_prefix(':').unwrap_or(k).to_string(), sym(v)?))).collect();
    // the inverted hash wins on duplicate keys; within it, the last key for a value wins
    pairs.iter().rev().find(|(_, v)| v == method).map(|(k, _)| k.clone()).or_else(|| {
        pairs.iter().find(|(k, _)| k == method).map(|(_, v)| v.clone())
    })
}

/// A parser `send`/`csend` (no literal block).
fn plain_call<'pr>(n: &Node<'pr>) -> Option<CallNode<'pr>> {
    // `/re/ =~ x` is parser `match_with_lvasgn`, not a send
    let mwl = |c: &CallNode| method_name(c) == "=~" && c.receiver().is_some_and(|r| r.as_regular_expression_node().is_some());
    n.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()) && !mwl(c))
}

fn is_not(c: &CallNode) -> bool {
    method_name(c) == "!" && !is_csend(c)
}

#[derive(Default)]
struct HasNext(bool);

impl<'pr> Visit<'pr> for HasNext {
    fn visit_next_node(&mut self, _node: &ruby_prism::NextNode<'pr>) {
        self.0 = true;
    }
}

impl Cop for InverseMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            self.on_block(cx, node, &call);
        }
        if is_not(&call) {
            self.on_send(cx, node, &call);
        }
    }
}

impl InverseMethods {
    fn on_send(&self, cx: &mut Cx, node: &Node, call: &CallNode) {
        let Some(recv) = call.receiver() else { return };
        // inverse_candidate?: (send $(call ...) :!), (send (any_block $(call recv meth)) :!),
        // (send (begin $(call ...)) :!)
        let (method_call, in_parens) = if let Some(c) = plain_call(&recv) {
            (c, false)
        } else if let Some(c) = recv.as_call_node() {
            if c.arguments().is_some() {
                return;
            }
            (c, false)
        } else if let Some(p) = recv.as_parentheses_node() {
            let Some(body) = p.body() else { return };
            let inner = match body.as_statements_node() {
                Some(s) if s.body().iter().count() == 1 => s.body().iter().next().unwrap(),
                Some(_) => return,
                None => body,
            };
            match plain_call(&inner) {
                Some(c) => (c, true),
                None => return,
            }
        } else {
            return;
        };
        let Some(lhs) = method_call.receiver() else { return };
        let method = method_name(&method_call);
        let Some(inverse) = inverse_of(cx, "InverseMethods", method) else { return };
        // negated?(node)
        if cx.parent().and_then(|p| p.as_call_node()).is_some_and(|p| method_name(&p) == "!") {
            return;
        }
        if is_csend(&method_call) && matches!(method, "<=" | ">=" | "<" | ">" | "any?" | "none?") {
            return;
        }
        let me = range(node);
        if cx.state::<Vec<(usize, usize)>>().iter().any(|&(a, b)| a <= me.0 && me.1 <= b) {
            return;
        }
        let rhs = send_args(&method_call);
        let camel = |n: &Node| {
            matches!(kind::of(n), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE) && CAMEL_CASE.is_match(node_text(n))
        };
        if matches!(method, "<=" | ">=" | "<" | ">") && (camel(&lhs) || (rhs.len() == 1 && camel(&rhs[0]))) {
            return;
        }
        let Some(sel) = method_call.message_loc() else { return };
        let not_start = call.message_loc().map_or(me.0, |l| l.start_offset());
        let mc = send_range(&method_call);
        let remove_end = matches!(method, "==" | "!=" | "=~" | "!~" | "<=" | ">=" | "<" | ">") || in_parens;
        let msg = format!("Use `{inverse}` instead of inverting `{method}`.");
        cx.add_fix(me.0, me.1, msg, |c| {
            c.remove(not_start, mc.0);
            c.replace(sel.start_offset(), sel.end_offset(), &inverse);
            if remove_end {
                c.remove(mc.1, me.1);
            }
        });
    }

    fn on_block(&self, cx: &mut Cx, node: &Node, call: &CallNode) {
        // (any_block $(call (...) $_) ... body)
        if call.receiver().is_none() || call.arguments().is_some() {
            return;
        }
        let block = call.block().unwrap().as_block_node().unwrap();
        let Some(body) = block.body() else { return };
        let last = match body.as_statements_node() {
            Some(s) => {
                let stmts: Vec<Node> = s.body().iter().collect();
                match stmts.len() {
                    0 => return,
                    1 => {
                        // a lone parenthesised statement list is a parser `begin` too
                        let only = s.body().iter().next().unwrap();
                        match only.as_parentheses_node().and_then(|p| p.body()) {
                            Some(pb) => match pb.as_statements_node() {
                                Some(ps) => ps.body().iter().last(),
                                None => Some(pb),
                            },
                            None if only.as_parentheses_node().is_some() => None,
                            None => Some(only),
                        }
                    }
                    _ => s.body().iter().last(),
                }
            }
            None => return,
        };
        let Some(last) = last else { return };
        let Some(inner) = plain_call(&last) else { return };
        let inner_name = method_name(&inner);
        let negating = inner_name == "!" && inner.arguments().is_none();
        let neq = matches!(inner_name, "!=" | "!~") && !is_csend(&inner) && inner.receiver().is_some();
        if !negating && !neq {
            return;
        }
        let method = method_name(call);
        let Some(inverse) = inverse_of(cx, "InverseBlocks", method) else { return };
        // negated?(node) && negated?(node.parent)
        let n = cx.ancestors.len();
        let is_not_at = |i: usize| cx.ancestors.get(i).and_then(|p| p.as_call_node()).is_some_and(|p| method_name(&p) == "!");
        if n >= 2 && is_not_at(n - 1) && is_not_at(n - 2) {
            return;
        }
        let mut v = HasNext::default();
        v.visit(node);
        if v.0 {
            return;
        }
        let inner_node = last;
        cx.state::<Vec<(usize, usize)>>().push(range(&inner_node));
        let Some(sel) = call.message_loc() else { return };
        let inner_sel = inner.message_loc();
        let inner_dot = inner.call_operator_loc().map(|d| d.start_offset());
        let inner_end = range(&inner_node).1;
        let me = range(node);
        let msg = format!("Use `{inverse}` instead of inverting `{method}`.");
        cx.add_fix(me.0, me.1, msg, |c| {
            c.replace(sel.start_offset(), sel.end_offset(), &inverse);
            let Some(isel) = inner_sel else { return };
            let s = text(&isel);
            if s == "!=" || s == "!~" {
                c.replace(isel.start_offset(), isel.end_offset(), &format!("={}", &s[1..]));
            } else {
                if let Some(d) = inner_dot {
                    c.remove(d, inner_end);
                }
                c.remove(isel.start_offset(), isel.end_offset());
            }
        });
    }
}
