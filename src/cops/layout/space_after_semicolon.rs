use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceAfterSemicolon";
pub static COP: SpaceAfterSemicolon = SpaceAfterSemicolon;
pub struct SpaceAfterSemicolon;

impl Cop for SpaceAfterSemicolon {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let style = helpers::space_after_punctuation::style_of(cx, "Layout/SpaceInsideBlockBraces");
        // A run of semicolons (`;;`) is not missing a space.
        helpers::space_after_punctuation::check(cx, &style, |t, next| {
            (t.kind == sys::PM_TOKEN_SEMICOLON && next.kind != sys::PM_TOKEN_SEMICOLON).then_some("semicolon")
        });
    }
}
