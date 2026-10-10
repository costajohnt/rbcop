//! Everything a cop file usually needs: `use crate::prelude::*;`.

pub use crate::ast::*;
pub use crate::config::value::{Map, Value};
pub use crate::cop::{Cop, Corrector, Cx};
pub use crate::helpers;
pub use crate::node_kind as kind;
pub use crate::source::{Comment, Token};
pub use regex::Regex;
pub use ruby_prism::Node;
/// Prism token kinds: `sys::PM_TOKEN_COMMA` etc., compared with `Token::kind`.
pub use ruby_prism_sys as sys;
pub use std::sync::LazyLock;
