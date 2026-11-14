use crate::helpers::sorbet_sigil::{Kind, investigate};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/HasSigil";
pub static COP: HasSigil = HasSigil;
pub struct HasSigil;

impl Cop for HasSigil {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        investigate(cx, Kind::Has);
    }
}
