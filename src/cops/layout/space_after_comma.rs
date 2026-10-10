use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceAfterComma";
pub static COP: SpaceAfterComma = SpaceAfterComma;
pub struct SpaceAfterComma;

impl Cop for SpaceAfterComma {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let style = helpers::space_after_punctuation::style_of(cx, "Layout/SpaceInsideHashLiteralBraces");
        helpers::space_after_punctuation::check(cx, &style, |t, next| {
            (t.kind == sys::PM_TOKEN_COMMA && next.kind != sys::PM_TOKEN_SEMICOLON).then_some("comma")
        });
    }
}
