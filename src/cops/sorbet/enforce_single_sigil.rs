use crate::helpers::sorbet_sigil::{SIGIL, leading_comments};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/EnforceSingleSigil";
pub static COP: EnforceSingleSigil = EnforceSingleSigil;
pub struct EnforceSingleSigil;

impl Cop for EnforceSingleSigil {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        if helpers::parser_tokens::tokens_empty(src, cx.facts.first_code) {
            return;
        }
        let sigils: Vec<(usize, usize)> =
            leading_comments(src).into_iter().filter(|&(s, e)| SIGIL.is_match(&src.text[s..e])).collect();
        // Each offense's corrector removes every sigil after the first: its line from column 0 up to
        // and including the character after it (`source_range(buffer, line, 0..last_column)`).
        let removals: Vec<(usize, usize)> = sigils
            .iter()
            .skip(1)
            .map(|&(s, e)| (src.line_start(src.line_of(s)), (e + 1).min(src.text.len())))
            .collect();
        for &(s, e) in sigils.iter().skip(1) {
            let removals = &removals;
            cx.add_fix(s, e, "Files must only contain one sigil", |k| {
                for &(b, e) in removals {
                    k.remove(b, e);
                }
            });
        }
    }
}
