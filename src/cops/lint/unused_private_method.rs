use crate::prelude::*;

pub const NAME: &str = "Lint/UnusedPrivateMethod";
pub static COP: UnusedPrivateMethod = UnusedPrivateMethod;
pub struct UnusedPrivateMethod;

// ponytail: `on_def` returns unless AllCops/UseProjectIndex (rubydex) is on, which rbcop does not
// support; without the index RuboCop reports nothing. Port the checks if rbcop grows a project index.
impl Cop for UnusedPrivateMethod {}
