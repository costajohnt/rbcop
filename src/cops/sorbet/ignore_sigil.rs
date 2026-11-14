use crate::helpers::sorbet_sigil::{Kind, investigate};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/IgnoreSigil";
pub static COP: IgnoreSigil = IgnoreSigil;
pub struct IgnoreSigil;

impl Cop for IgnoreSigil {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        investigate(cx, Kind::Minimum("ignore"));
    }
}
