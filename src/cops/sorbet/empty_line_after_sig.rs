use crate::helpers::sorbet_signature::{as_send, index_of, signature, siblings};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/EmptyLineAfterSig";
pub static COP: EmptyLineAfterSig = EmptyLineAfterSig;
pub struct EmptyLineAfterSig;

/// sig_or_signable_method_definition?: a def, a receiver-less `attr*` send or another signature.
fn signable(node: &Node) -> bool {
    node.as_def_node().is_some()
        || signature(node).is_some()
        || as_send(node).is_some_and(|c| {
            c.receiver().is_none() && matches!(c.name().as_slice(), b"attr" | b"attr_reader" | b"attr_writer" | b"attr_accessor")
        })
}

fn rubocop_line(line: &str) -> bool {
    helpers::ruby_string::strip(line).starts_with("# rubocop:")
}

impl Cop for EmptyLineAfterSig {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if signature(node).is_none() {
            return;
        }
        let Some(sibs) = siblings(cx.ancestors.last()) else { return };
        let (sig_start, sig_end) = helpers::node_types::range(node);
        let Some(next) = index_of(&sibs, (sig_start, sig_end)).and_then(|i| sibs.get(i + 1)).filter(|n| signable(n)) else {
            return;
        };
        // lines_between(sig, definition)
        let text = cx.src.text;
        let between = &text[sig_end..next.location().start_offset()];
        let begin_offset = between.find('\n').unwrap_or(0);
        let end_offset = between.rfind('\n').unwrap_or(between.len().wrapping_sub(1));
        let (s, e) = (sig_end + begin_offset + 1, sig_end.wrapping_add(end_offset).wrapping_add(1));
        if e <= s {
            return;
        }
        let source = &text[s..e];
        if !source.contains('\n') || source.split_inclusive('\n').all(rubocop_line) {
            return;
        }
        let (rubocop_lines, other_lines): (Vec<&str>, Vec<&str>) = source.split_inclusive('\n').partition(|l| rubocop_line(l));
        static LEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A\n+").unwrap());
        static BLANKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{2,}").unwrap());
        let moved = (!other_lines.is_empty()).then(|| {
            let joined = other_lines.concat();
            BLANKS.replace_all(&LEADING.replace(&joined, ""), "\n").into_owned()
        });
        let (ls, le) = helpers::whole_lines::range_by_whole_lines(text, sig_start, sig_end, false);
        let kept = rubocop_lines.concat();
        cx.add_fix(s, e, "Extra empty line or comment detected", |c| {
            if let Some(m) = &moved {
                c.insert_before(ls, le, m);
            }
            c.replace(s, e, &kept);
        });
    }
}
