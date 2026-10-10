use crate::prelude::*;
use ruby_prism::DefNode;
use std::collections::HashSet;

pub const NAME: &str = "Layout/DefEndAlignment";
pub static COP: DefEndAlignment = DefEndAlignment;
pub struct DefEndAlignment;

/// Defs already checked through their modifier send (`ignore_node`), by start offset.
#[derive(Default)]
struct Ignored(HashSet<usize>);

/// `def_modifier`: `private def ...`, `memoize private def ...` (receiverless sends whose first
/// argument chain ends in a def).
fn def_modifier<'pr>(node: &Node<'pr>) -> Option<DefNode<'pr>> {
    let call = node.as_call_node()?;
    if call.receiver().is_some() || is_csend(&call) {
        return None;
    }
    let arg = send_args(&call).into_iter().next()?;
    match arg.as_def_node() {
        Some(d) => Some(d),
        None => def_modifier(&arg),
    }
}

impl Cop for DefEndAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let style = cx.cfg.str("EnforcedStyleAlignWith").unwrap_or("start_of_line").to_string();
        let (def, ranges) = if kind::of(node) == kind::CALL_NODE {
            let Some(def) = def_modifier(node) else { return };
            let kw = def.def_keyword_loc();
            let ranges = vec![
                ("def", (kw.start_offset(), kw.end_offset())),
                ("start_of_line", (node.location().start_offset(), kw.end_offset())),
            ];
            (def, ranges)
        } else {
            let def = node.as_def_node().unwrap();
            let kw = def.def_keyword_loc();
            (def, vec![(style.as_str(), (kw.start_offset(), kw.end_offset()))])
        };
        let start = def.location().start_offset();
        let ignored = &mut cx.state::<Ignored>().0;
        if ignored.contains(&start) {
            return;
        }
        if kind::of(node) == kind::CALL_NODE {
            ignored.insert(start);
        }
        let Some(end) = def.end_keyword_loc() else { return };
        helpers::end_keyword_alignment::check(cx, (end.start_offset(), end.end_offset()), &ranges, &style);
    }
}
