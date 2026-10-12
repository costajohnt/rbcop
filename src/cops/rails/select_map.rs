use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/SelectMap";
pub static COP: SelectMap = SelectMap;
pub struct SelectMap;

impl Cop for SelectMap {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let map = method_name(&call);
        if !matches!(map, "map" | "collect") {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        let column = node_text(first).strip_prefix("&:").unwrap_or(node_text(first)).to_string();
        // node.descendants: `select` calls whose one argument names the column.
        struct Selects<'c>(&'c str, Vec<(usize, usize)>);
        impl<'pr> Visit<'pr> for Selects<'_> {
            fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
                let a = send_args(c);
                if method_name(c) == "select" && a.len() == 1 {
                    let arg = if a[0].as_symbol_node().is_some() {
                        node_text(&a[0]).strip_prefix(':').map(String::from)
                    } else {
                        helpers::rails::str_value(&a[0])
                    };
                    if arg.as_deref() == Some(self.0) {
                        self.1.push((c.location().start_offset(), c.location().end_offset()));
                    }
                }
                ruby_prism::visit_call_node(self, c);
            }
        }
        let mut sel = Selects(&column, Vec::new());
        if let Some(r) = call.receiver() {
            sel.visit(&r);
        }
        for a in &args {
            sel.visit(a);
        }
        let [select_range] = sel.1.as_slice() else { return };
        // receiver_chain?: follow receivers (through parentheses) down to the select.
        let mut current = call.receiver();
        let mut select: Option<ruby_prism::CallNode> = None;
        let mut parent: Option<ruby_prism::CallNode> = Some(call.as_node().as_call_node().unwrap());
        let mut parens_parent = false;
        while let Some(cur) = current.take() {
            let cur = match cur.as_parentheses_node() {
                Some(p) => {
                    parens_parent = true;
                    match p.body().map(|b| helpers::rails::body_statements(Some(b))) {
                        Some(mut v) if !v.is_empty() => v.pop().unwrap(),
                        _ => break,
                    }
                }
                None => cur,
            };
            let Some(c) = cur.as_call_node() else { break };
            if (c.location().start_offset(), c.location().end_offset()) == *select_range {
                select = Some(c);
                break;
            }
            parens_parent = false;
            current = c.receiver();
            parent = Some(c);
        }
        let Some(select) = select else { return };
        let preferred = format!("pluck(:{column})");
        let ssel = select.message_loc().unwrap();
        let (_, e) = send_range(&call);
        let s = ssel.start_offset();
        let msg = format!("Use `{preferred}` instead of `select` with `{map}`.");
        // autocorrect: the select's parent (the next call in the chain) has a dot.
        let parent_dot = if parens_parent { None } else { parent.and_then(|p| p.call_operator_loc()).map(|d| (d.start_offset(), d.end_offset())) };
        let select_end = select.location().end_offset();
        let msel = call.message_loc().unwrap().start_offset();
        let recv = select.receiver().map(|r| node_text(&r).to_string());
        let (ns, ne) = send_range(&call);
        if parent_dot.is_none() && recv.is_none() {
            // `select_node.receiver.source` raises: the offense is lost.
            return;
        }
        cx.add_fix(s, e, msg, |c| match parent_dot {
            Some((ds, de)) => {
                c.remove(ds, de);
                c.remove(s, select_end);
                c.replace(msel, e, &preferred);
            }
            None => c.replace(ns, ne, &format!("{}.{preferred}", recv.unwrap())),
        });
    }
}
