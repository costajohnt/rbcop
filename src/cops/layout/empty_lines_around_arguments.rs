use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLinesAroundArguments";
pub static COP: EmptyLinesAroundArguments = EmptyLinesAroundArguments;
pub struct EmptyLinesAroundArguments;

impl Cop for EmptyLinesAroundArguments {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        let call = node.as_call_node().unwrap();
        let (start, end) = send_range(&call);
        let args = send_args(&call);
        if src.line_of(start) == src.line_of(end) || args.is_empty() {
            return;
        }
        // receiver_and_method_call_on_different_lines? (no selector, e.g. `a.()`, counts as different)
        if let Some(r) = call.receiver()
            && call.message_loc().is_none_or(|m| src.line_of(r.location().end_offset()) != src.line_of(m.start_offset()))
        {
            return;
        }
        let mut starts: Vec<usize> = args.iter().map(|a| a.location().start_offset()).collect();
        // loc.end is the closing `)`; index sends (`a[...]`) have none in parser.
        if let Some(c) = call.closing_loc().filter(|c| c.as_slice() == b")") {
            starts.push(c.start_offset());
        }
        for s in starts {
            // range_with_surrounding_space(whitespace: true, side: :left)
            let bytes = src.text.as_bytes();
            let mut pos = s;
            while pos > 0 && helpers::parser_tokens::is_ruby_space(bytes[pos - 1]) {
                pos -= 1;
            }
            let last = src.line_of(s);
            if last - src.line_of(pos) > 1 {
                let (a, b) = (src.line_start(last - 1), src.line_start(last));
                cx.add_fix(a, b, "Empty line detected around arguments.", |c| c.remove(a, b));
            }
        }
    }
}
