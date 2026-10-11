use crate::prelude::*;

pub const NAME: &str = "Layout/InitialIndentation";
pub static COP: InitialIndentation = InitialIndentation;
pub struct InitialIndentation;

impl Cop for InitialIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let tokens = helpers::parser_tokens::translated(src);
        let Some(t) = tokens.iter().find(|t| !src.text[t.start..].starts_with('#')) else { return };
        // Only spaces/tabs to the left count (a BOM gives a non-zero column but no space):
        // range_with_surrounding_space(newlines: false) moves over `/[ \t]/` only, not Ruby's `\s`.
        if matches!(src.text.as_bytes()[..t.start].last(), Some(b' ' | b'\t')) {
            let o = helpers::range_help::Surrounding {
                side: helpers::range_help::Side::Left,
                newlines: false,
                ..Default::default()
            };
            let (space, _) = helpers::range_help::range_with_surrounding_space(src.text, t.start, t.end, o);
            let first = t.start;
            cx.add_fix(t.start, t.end, "Indentation of first line in file detected.", |c| c.remove(space, first));
        }
    }
}
