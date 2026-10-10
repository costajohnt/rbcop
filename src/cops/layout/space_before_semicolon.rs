use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceBeforeSemicolon";
pub static COP: SpaceBeforeSemicolon = SpaceBeforeSemicolon;
pub struct SpaceBeforeSemicolon;

impl Cop for SpaceBeforeSemicolon {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        helpers::space_before_punctuation::check(cx, sys::PM_TOKEN_SEMICOLON, "semicolon");
    }
}
