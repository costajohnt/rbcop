use crate::helpers::sorbet_signature::{each_call, signature};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/CheckedTrueInSignature";
pub static COP: CheckedTrueInSignature = CheckedTrueInSignature;
pub struct CheckedTrueInSignature;

const MESSAGE: &str = "Using `checked(true)` in a method signature definition is not allowed. `checked(true)` is the default behavior for modules/classes with runtime checks enabled. To enable typechecking at runtime for this module, regardless of global settings, `include(WaffleCone::RuntimeChecks)` to this module and set other methods to `checked(false)`.";

impl Cop for CheckedTrueInSignature {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if signature(node).is_none() {
            return;
        }
        // offending_node(node).first: `(send _ :checked (true))`.
        let mut error = None;
        each_call(node, |c| {
            if error.is_none()
                && !is_csend(c)
                && c.name().as_slice() == b"checked"
                && matches!(send_args(c).as_slice(), [a] if a.as_true_node().is_some())
            {
                let sel = c.message_loc().map(|l| l.start_offset());
                error = Some((c.location().start_offset(), sel, c.closing_loc().map(|l| l.start_offset())));
            }
        });
        // Without parentheses `loc.end` is nil and the cop raises.
        let Some((start, Some(sel), Some(close))) = error else { return };
        // source_range(buffer, line, selector.begin_pos..end.begin_pos) takes the absolute
        // positions as columns: the range is shifted right by the line's start.
        let text = cx.src.text;
        let chars = |b: usize| text[..b].chars().count();
        let line_begin = chars(cx.src.line_start(cx.src.line_of(start)));
        let (sel, close) = (chars(sel), chars(close));
        let begin = line_begin + sel;
        let end = begin + (close - sel + 1);
        // ponytail: a range past the end of the file (RuboCop reports it there) is dropped.
        let byte = |c: usize| text.char_indices().map(|(i, _)| i).chain([text.len()]).nth(c);
        if let (Some(b), Some(e)) = (byte(begin), byte(end)) {
            cx.add(b, e, MESSAGE);
        }
    }
}
