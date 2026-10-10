use crate::prelude::*;

pub const NAME: &str = "Lint/ArgumentMismatch";
pub static COP: ArgumentMismatch = ArgumentMismatch;
pub struct ArgumentMismatch;

// ponytail: every check needs AllCops/UseProjectIndex (rubydex), which rbcop does not support;
// without the index RuboCop reports nothing. Port the checks if rbcop grows a project index.
impl Cop for ArgumentMismatch {}
