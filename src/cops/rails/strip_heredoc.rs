use crate::prelude::*;

pub const NAME: &str = "Rails/StripHeredoc";
pub static COP: StripHeredoc = StripHeredoc;
pub struct StripHeredoc;

impl Cop for StripHeredoc {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.3, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "strip_heredoc" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let opening = match &recv {
            Node::StringNode { .. } => recv.as_string_node().unwrap().opening_loc(),
            Node::InterpolatedStringNode { .. } => recv.as_interpolated_string_node().unwrap().opening_loc(),
            _ => return,
        };
        let Some(open) = opening.filter(|o| o.as_slice().starts_with(b"<<")) else { return };
        let (hs, he) = (open.start_offset(), open.end_offset());
        let src = text(&open);
        let rest = src.trim_start_matches("<<");
        let rest = rest.strip_prefix(['-', '~']).unwrap_or(rest);
        let squiggly = format!("<<~{rest}");
        let dot = call.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()));
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Use squiggly heredoc (`<<~`) instead of `strip_heredoc`.", |c| {
            c.replace(hs, he, &squiggly);
            if let Some((ds, de)) = dot {
                c.remove(ds, de);
            }
            c.remove(ss, se);
        });
    }
}
