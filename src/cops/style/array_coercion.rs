use crate::helpers::if_node::{Else, parser_end, parts};
use crate::prelude::*;

pub const NAME: &str = "Style/ArrayCoercion";
pub static COP: ArrayCoercion = ArrayCoercion;
pub struct ArrayCoercion;

impl Cop for ArrayCoercion {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE, kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(arr) = node.as_array_node() {
            // square_brackets? and (array (splat $_))
            if arr.opening_loc().is_none_or(|o| o.as_slice() != b"[") {
                return;
            }
            let mut elems = arr.elements().iter();
            let (Some(e), None) = (elems.next(), elems.next()) else { return };
            let Some(arg) = e.as_splat_node().and_then(|s| s.expression()) else { return };
            let src = node_text(&arg);
            let loc = node.location();
            let replacement = format!("Array({src})");
            cx.add_fix(loc.start_offset(), loc.end_offset(), format!("Use `Array({src})` instead of `[*{src}]`."), |c| {
                c.replace(loc.start_offset(), loc.end_offset(), &replacement)
            });
            return;
        }
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        // (if (send (lvar $_) :is_a? (const nil? :Array)) nil? (lvasgn $_ (array (lvar $_))))
        let (cond, body, els) = parts(node);
        let (then_part, else_part) = if node.as_unless_node().is_some() {
            (els.map(|_| ()), body)
        } else {
            let else_body = match els {
                Some(Else::Body(b)) => b,
                _ => return,
            };
            (body.map(|_| ()), else_body)
        };
        if then_part.is_some() {
            return;
        }
        let Some(asgn) = else_part.and_then(|b| b.single()) else { return };
        let Some(call) = cond.as_call_node() else { return };
        if method_name(&call) != "is_a?" || is_csend(&call) || call.block().is_some() {
            return;
        }
        let Some(var_a) = call.receiver().and_then(|r| r.as_local_variable_read_node()) else { return };
        let args = call_args(&call);
        if args.len() != 1 || args[0].as_constant_read_node().is_none_or(|c| c.name().as_slice() != b"Array") {
            return;
        }
        let Some(w) = asgn.as_local_variable_write_node() else { return };
        let Some(arr) = w.value().as_array_node() else { return };
        let mut elems = arr.elements().iter();
        let (Some(e), None) = (elems.next(), elems.next()) else { return };
        let Some(var_c) = e.as_local_variable_read_node() else { return };
        let (a, b, c) = (var_a.name().as_slice(), w.name().as_slice(), var_c.name().as_slice());
        if a != b || c != b {
            return;
        }
        let var = String::from_utf8_lossy(a).into_owned();
        let (start, end) = (node.location().start_offset(), parser_end(node));
        let replacement = format!("{var} = Array({var})");
        cx.add_fix(start, end, format!("Use `Array({var})` instead of explicit `Array` check."), |c| {
            c.replace(start, end, &replacement)
        });
    }
}
