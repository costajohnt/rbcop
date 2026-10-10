use crate::prelude::*;

pub const NAME: &str = "Lint/SuperArgumentMismatch";
pub static COP: SuperArgumentMismatch = SuperArgumentMismatch;
pub struct SuperArgumentMismatch;

// ponytail: every check needs AllCops/UseProjectIndex (rubydex), which rbcop does not support;
// without the index RuboCop reports nothing. Port the checks if rbcop grows a project index.
impl Cop for SuperArgumentMismatch {}
