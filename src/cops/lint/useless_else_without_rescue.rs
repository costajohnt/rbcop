use crate::prelude::*;
use crate::runner::useless_else;

pub const NAME: &str = "Lint/UselessElseWithoutRescue";
pub static COP: UselessElseWithoutRescue = UselessElseWithoutRescue;
pub struct UselessElseWithoutRescue;

impl Cop for UselessElseWithoutRescue {
    fn ruby_range(&self) -> (f64, f64) {
        (0.0, 2.5)
    }

    /// The parser gem's `useless_else` warning (targets up to 2.5) is a Prism error the runner
    /// tolerates for those targets (runner::useless_else).
    fn on_file(&self, cx: &mut Cx) {
        let result = ruby_prism::parse_with_version(cx.src.text.as_bytes(), cx.src.version, !cx.src.binary);
        // Prism's error spans the whole else clause; parser's diagnostic is the `else` keyword.
        let found: Vec<usize> =
            result.errors().filter(|e| useless_else(e, &result.node(), cx.src.target_ruby, cx.src.text)).map(|e| e.location().start_offset()).collect();
        for start in found {
            cx.add(start, start + "else".len(), "`else` without `rescue` is useless.");
        }
    }
}
