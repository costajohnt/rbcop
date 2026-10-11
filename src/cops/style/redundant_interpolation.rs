use crate::prelude::*;
use helpers::select_by::plain_send;
use helpers::node_types::is_operator_method;

pub const NAME: &str = "Style/RedundantInterpolation";
pub static COP: RedundantInterpolation = RedundantInterpolation;
pub struct RedundantInterpolation;

/// `variable? || reference?`
fn is_variable(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::LOCAL_VARIABLE_READ_NODE
            | kind::IT_LOCAL_VARIABLE_READ_NODE
            | kind::INSTANCE_VARIABLE_READ_NODE
            | kind::CLASS_VARIABLE_READ_NODE
            | kind::GLOBAL_VARIABLE_READ_NODE
            | kind::NUMBERED_REFERENCE_READ_NODE
            | kind::BACK_REFERENCE_READ_NODE
    )
}

impl Cop for RedundantInterpolation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let s = node.as_interpolated_string_node().unwrap();
        let parts: Vec<_> = s.parts().iter().collect();
        let [part] = parts.as_slice() else { return };
        // `implicit_concatenation?` / `embedded_in_percent_array?`
        if let Some(p) = cx.ancestors.last() {
            if p.as_interpolated_string_node().is_some() {
                return;
            }
            if p.as_array_node().is_some_and(|a| helpers::percent_literal::percent_type(&a).is_some()) {
                return;
            }
        }
        let (Some(open), Some(close)) = (s.opening_loc(), s.closing_loc()) else { return };
        let loc = node.location();
        let (ns, ne) = (loc.start_offset(), loc.end_offset());
        let msg = "Prefer `to_s` over string interpolation.";

        if let Some(v) = part.as_embedded_variable_node() {
            let replacement = format!("{}.to_s", node_text(&v.variable()));
            cx.add_fix(ns, ne, msg, |c| c.replace(ns, ne, &replacement));
            return;
        }
        let Some(e) = part.as_embedded_statements_node() else { return };
        let children: Vec<Node> = e.statements().map(|s| s.body().iter().collect()).unwrap_or_default();
        if cx.config.target_ruby > 2.7 && children.iter().any(|c| c.as_match_required_node().is_some()) {
            return;
        }
        let single = match children.as_slice() {
            [child] if is_variable(child) => Some(node_text(child).to_string()),
            [child] => plain_send(child).filter(|c| !is_operator_method(method_name(c))).map(|call| {
                let mut args: Vec<&str> = call_args(&call).iter().map(node_text).collect();
                if let Some(b) = call.block() {
                    args.push(node_text(&b));
                }
                let parenthesized = call.closing_loc().is_some_and(|l| l.as_slice() == b")");
                match call.message_loc() {
                    Some(sel) if !args.is_empty() && !parenthesized => {
                        let src: &str = cx.src.text;
                        format!("{}({})", &src[child.location().start_offset()..sel.end_offset()], args.join(", "))
                    }
                    _ => node_text(child).to_string(),
                }
            }),
            _ => None,
        };
        if let Some(source) = single {
            let replacement = format!("{source}.to_s");
            cx.add_fix(ns, ne, msg, |c| c.replace(ns, ne, &replacement));
            return;
        }
        let (o, cl) = ((open.start_offset(), open.end_offset()), (close.start_offset(), close.end_offset()));
        let (eo, ec) = (e.opening_loc(), e.closing_loc());
        let (eo, ec) = ((eo.start_offset(), eo.end_offset()), (ec.start_offset(), ec.end_offset()));
        cx.add_fix(ns, ne, msg, |c| {
            c.replace(o.0, o.1, "");
            c.replace(cl.0, cl.1, "");
            c.replace(eo.0, eo.1, "(");
            c.replace(ec.0, ec.1, ").to_s");
        });
    }
}
