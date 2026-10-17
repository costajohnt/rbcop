use crate::prelude::*;

pub const NAME: &str = "Rails/FreezeTime";
pub static COP: FreezeTime = FreezeTime;
pub struct FreezeTime;

/// A parser send/csend (no literal block): (receiver, method, has arguments).
fn parts<'pr>(n: &Node<'pr>) -> Option<(Option<Node<'pr>>, String, bool)> {
    let c = n.as_call_node()?;
    if c.block().is_some_and(|b| b.as_block_node().is_some()) {
        return None;
    }
    Some((c.receiver(), method_name(&c).to_string(), !send_args(&c).is_empty()))
}

fn current_time(recv: &Node, method: &str) -> bool {
    if !matches!(method, "now" | "new" | "current") {
        return false;
    }
    match recv.as_call_node() {
        // send_type? (a csend is not): (send (const {nil? cbase} :Time) :zone)
        Some(c) => {
            !is_csend(&c)
                && c.block().is_none()
                && method_name(&c) == "zone"
                && send_args(&c).is_empty()
                && c.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Time"))
        }
        None => {
            helpers::parser_parent::is_top_const(recv, "Time") || helpers::parser_parent::is_top_const(recv, "DateTime")
        }
    }
}

impl Cop for FreezeTime {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.2
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "travel_to" {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        // child_node, method_name, time_argument = *node.first_argument&.children
        let Some((Some(child), method, has_args)) = parts(first) else { return };
        if has_args {
            return;
        }
        let with_convert = || {
            matches!(method.as_str(), "to_time" | "in_time_zone")
                && parts(&child).is_some_and(|(r, m, a)| !a && r.is_some_and(|r| current_time(&r, &m)))
        };
        if !current_time(&child, &method) && !with_convert() {
            return;
        }
        let replacement = match args.last() {
            Some(l) if l.as_block_argument_node().is_some() => format!("freeze_time({})", node_text(l)),
            _ => "freeze_time".to_string(),
        };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Use `freeze_time` instead of `travel_to`.", |c| c.replace(s, e, &replacement));
    }
}
