use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceBeforeComma";
pub static COP: SpaceBeforeComma = SpaceBeforeComma;
pub struct SpaceBeforeComma;

impl Cop for SpaceBeforeComma {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        helpers::space_before_punctuation::check(cx, sys::PM_TOKEN_COMMA, "comma");
    }
}
