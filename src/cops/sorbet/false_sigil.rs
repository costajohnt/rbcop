use crate::helpers::sorbet_sigil::{Kind, investigate};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/FalseSigil";
pub static COP: FalseSigil = FalseSigil;
pub struct FalseSigil;

impl Cop for FalseSigil {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        investigate(cx, Kind::Minimum("false"));
    }
}
