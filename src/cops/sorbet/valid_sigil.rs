use crate::helpers::sorbet_sigil::{Kind, investigate};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ValidSigil";
pub static COP: ValidSigil = ValidSigil;
pub struct ValidSigil;

impl Cop for ValidSigil {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        investigate(cx, Kind::Valid);
    }
}
