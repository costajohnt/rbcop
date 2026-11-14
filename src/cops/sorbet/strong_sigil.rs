use crate::helpers::sorbet_sigil::{Kind, investigate};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/StrongSigil";
pub static COP: StrongSigil = StrongSigil;
pub struct StrongSigil;

impl Cop for StrongSigil {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        investigate(cx, Kind::Minimum("strong"));
    }
}
