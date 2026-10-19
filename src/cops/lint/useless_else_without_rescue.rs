use crate::prelude::*;
use crate::runner::useless_elses;

pub const NAME: &str = "Lint/UselessElseWithoutRescue";
pub static COP: UselessElseWithoutRescue = UselessElseWithoutRescue;
pub struct UselessElseWithoutRescue;

impl Cop for UselessElseWithoutRescue {
    fn ruby_range(&self) -> (f64, f64) {
        (0.0, 2.5)
    }

    /// The parser gem's `useless_else` warning (targets up to 2.5), an `else` the runner blanks out
    /// before Prism parses (runner::useless_elses).
    fn on_file(&self, cx: &mut Cx) {
        let (result, tokens) =
            ruby_prism::parse_with_version_and_tokens(cx.src.text.as_bytes(), cx.src.version, !cx.src.binary);
        for start in useless_elses(&tokens, result.errors().next().is_some(), cx.src.target_ruby) {
            cx.add(start, start + "else".len(), "`else` without `rescue` is useless.");
        }
    }
}
