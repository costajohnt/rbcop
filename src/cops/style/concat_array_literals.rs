use crate::prelude::*;

pub const NAME: &str = "Style/ConcatArrayLiterals";
pub static COP: ConcatArrayLiterals = ConcatArrayLiterals;
pub struct ConcatArrayLiterals;

fn is_percent(arr: &ruby_prism::ArrayNode) -> bool {
    arr.opening_loc().is_some_and(|o| text(&o).starts_with('%'))
}

impl Cop for ConcatArrayLiterals {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "concat" || call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
            return;
        }
        let args = call_args(&call);
        if args.is_empty() || !args.iter().all(|a| a.as_array_node().is_some()) {
            return;
        }
        let arrays: Vec<_> = args.iter().map(|a| a.as_array_node().unwrap()).collect();
        let Some(sel) = call.message_loc() else { return };
        let (os, oe) = (sel.start_offset(), send_range(&call).1);
        let current = cx.src.slice(os, oe);
        let binary = cx.src.binary;
        // preferred_method; None when a percent literal holds a non-basic element (`value` raises).
        let preferred = || -> Option<String> {
            let mut parts = Vec::new();
            for arr in &arrays {
                for child in arr.elements().iter() {
                    if !is_percent(arr) {
                        parts.push(node_text(&child).to_string());
                    } else if let Some(s) = child.as_string_node() {
                        parts.push(if binary {
                            helpers::ruby_string::inspect_binary(s.unescaped())
                        } else {
                            helpers::ruby_string::inspect(s.unescaped())
                        });
                    } else {
                        parts.push(helpers::ruby_string::symbol_inspect(child.as_symbol_node()?.unescaped()));
                    }
                }
            }
            Some(format!("push({})", parts.join(", ")))
        };
        let use_percent = arrays.iter().any(is_percent);
        let basic = arrays.iter().filter(|a| is_percent(a)).all(|a| {
            a.elements().iter().all(|c| c.as_string_node().is_some() || c.as_symbol_node().is_some())
        });
        let prefer = if !use_percent || basic { preferred() } else { None };
        let message = match &prefer {
            Some(p) => format!("Use `{p}` instead of `{current}`."),
            None => format!(
                "Use `push` with elements as arguments without array brackets instead of `{current}`."
            ),
        };
        enum Fix {
            None,
            Replace(String),
            Brackets(Vec<(usize, usize)>),
        }
        let fix = if use_percent || arrays.iter().any(|a| a.elements().iter().next().is_none()) {
            prefer.map_or(Fix::None, Fix::Replace)
        } else {
            let mut ranges = Vec::new();
            for a in &arrays {
                if let (Some(o), Some(c)) = (a.opening_loc(), a.closing_loc()) {
                    ranges.push((o.start_offset(), o.end_offset()));
                    ranges.push((c.start_offset(), c.end_offset()));
                }
            }
            Fix::Brackets(ranges)
        };
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(os, oe, message, |c| match &fix {
            Fix::None => {}
            Fix::Replace(p) => c.replace(os, oe, p),
            Fix::Brackets(ranges) => {
                c.replace(ss, se, "push");
                for &(s, e) in ranges {
                    c.remove(s, e);
                }
            }
        });
    }
}
