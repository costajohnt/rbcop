use crate::helpers::sorbet_sigil::{Kind, investigate};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/TrueSigil";
pub static COP: TrueSigil = TrueSigil;
pub struct TrueSigil;

impl Cop for TrueSigil {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        investigate(cx, Kind::Minimum("true"));
    }
}
