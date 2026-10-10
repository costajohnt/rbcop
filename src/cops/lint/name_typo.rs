use crate::prelude::*;

pub const NAME: &str = "Lint/NameTypo";
pub static COP: NameTypo = NameTypo;
pub struct NameTypo;

// ponytail: every check needs AllCops/UseProjectIndex (rubydex), which rbcop does not support;
// without the index RuboCop reports nothing. Port the checks (with helpers::did_you_mean) if
// rbcop grows a project index.
impl Cop for NameTypo {}
