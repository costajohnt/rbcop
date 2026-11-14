use crate::helpers::sorbet_sigil::{Kind, investigate};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/StrictSigil";
pub static COP: StrictSigil = StrictSigil;
pub struct StrictSigil;

impl Cop for StrictSigil {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        investigate(cx, Kind::Minimum("strict"));
    }
}
